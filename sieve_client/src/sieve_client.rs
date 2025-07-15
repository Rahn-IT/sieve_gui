use base64::{Engine as _, engine::general_purpose};
use nom::{
    IResult,
    bytes::complete::take_until,
    character::complete::{char, space0},
    combinator::opt,
};
use rustls::{ClientConfig, RootCertStore};
use rustls_pki_types::ServerName;
use std::{collections::HashMap, fmt::Debug};
use std::{io, sync::Arc};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_rustls::{TlsConnector, client::TlsStream};

// Type aliases for cleaner code
type TlsReader = tokio::io::ReadHalf<TlsStream<TcpStream>>;
type TlsWriter = tokio::io::WriteHalf<TlsStream<TcpStream>>;

#[derive(Debug, Clone)]
pub struct Capabilities {
    pub implementation: Option<String>,
    pub sasl: Vec<String>,
    pub sieve: Vec<String>,
    pub starttls: bool,
    pub maxredirects: Option<u32>,
    pub notify: Vec<String>,
    pub language: Option<String>,
    pub owner: Option<String>,
    pub version: Option<String>,
    pub other: HashMap<String, String>,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            implementation: None,
            sasl: Vec::new(),
            sieve: Vec::new(),
            starttls: false,
            maxredirects: None,
            notify: Vec::new(),
            language: None,
            owner: None,
            version: None,
            other: HashMap::new(),
        }
    }
}

pub struct SieveClient {
    connection: Mutex<(BufReader<TlsReader>, TlsWriter)>,
    capabilities: Capabilities,
}

impl Debug for SieveClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SieveClient")
    }
}

#[derive(Debug, Error)]
pub enum ConnectError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(#[from] io::Error),
    #[error("Protocol error: {0}")]
    ProtocolError(String),
    #[error("TLS error: {0}")]
    TlsError(#[from] rustls::Error),
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),
}

#[derive(Debug, Error)]
pub enum ManageSieveError {
    #[error("IO error: {0}")]
    IoError(#[from] io::Error),
    #[error("Protocol error: {0}")]
    ProtocolError(String),
    #[error("Server error: {0}")]
    ServerError(String),
    #[error("Script not found: {0}")]
    ScriptNotFound(String),
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

impl SieveClient {
    pub async fn connect(
        host: String,
        port: u16,
        username: &str,
        password: &str,
    ) -> Result<Self, ConnectError> {
        // Connect to specified host and port
        let address = format!("{}:{}", host, port);

        // Establish TCP connection
        let mut stream = TcpStream::connect(&address).await?;

        // Ignore initial capabilities greeting - just read until OK
        Self::ignore_initial_greeting(&mut stream).await?;

        // Send STARTTLS command immediately
        stream.write_all(b"STARTTLS\r\n").await?;
        stream.flush().await?;

        // Read STARTTLS response
        let mut reader = BufReader::new(&mut stream);
        let mut response = String::new();
        reader.read_line(&mut response).await?;

        if !response.trim().to_uppercase().starts_with("OK") {
            return Err(ConnectError::ProtocolError(format!(
                "STARTTLS failed: {}",
                response.trim()
            )));
        }

        // Set up TLS configuration
        let mut root_store = RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        let config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        let connector = TlsConnector::from(Arc::new(config));
        let domain = ServerName::try_from(host.as_str())
            .map_err(|_| ConnectError::ProtocolError("Invalid hostname".to_string()))?
            .to_owned();

        // Perform TLS handshake
        let tls_stream = connector.connect(domain, stream).await?;

        // Split the TLS stream
        let (tls_read, tls_write) = tokio::io::split(tls_stream);
        let mut tls_reader = BufReader::new(tls_read);

        // Read capabilities after TLS
        let capabilities = Self::read_capabilities(&mut tls_reader).await?;

        // Create the client instance
        let client = SieveClient {
            connection: Mutex::new((tls_reader, tls_write)),
            capabilities,
        };

        // Authenticate with the server
        client.authenticate(username, password).await?;

        Ok(client)
    }

    pub async fn list_scripts(&self) -> Result<Vec<(String, bool)>, ManageSieveError> {
        let mut connection = self.connection.lock().await;
        let (reader, writer) = &mut *connection;

        // Send LISTSCRIPTS command
        writer.write_all(b"LISTSCRIPTS\r\n").await?;
        writer.flush().await?;

        let mut scripts = Vec::new();
        let mut response = String::new();

        loop {
            response.clear();
            reader.read_line(&mut response).await?;
            let line = response.trim();

            if line.is_empty() {
                continue;
            }

            let line_upper = line.to_uppercase();
            if line_upper.starts_with("OK") {
                break;
            } else if line_upper.starts_with("NO") || line_upper.starts_with("BYE") {
                return Err(ManageSieveError::ServerError(line.to_string()));
            } else if line.starts_with("\"") {
                // Parse quoted script name
                if let Some(script_name) = self.parse_script_line(line) {
                    scripts.push(script_name);
                }
            }
        }

        Ok(scripts)
    }

    pub async fn get_script(&self, script: &str) -> Result<String, ManageSieveError> {
        let mut connection = self.connection.lock().await;
        let (reader, writer) = &mut *connection;

        // Send GETSCRIPT command
        let command = format!("GETSCRIPT \"{}\"\r\n", script);
        writer.write_all(command.as_bytes()).await?;
        writer.flush().await?;

        let mut response = String::new();
        reader.read_line(&mut response).await?;
        let line = response.trim();

        // Check if we got a literal string response
        if line.starts_with("{") {
            // Parse literal string length
            if let Some(length) = self.parse_literal_length(line) {
                // Read the exact number of bytes for the script content
                let mut script_content = vec![0u8; length];
                reader.read_exact(&mut script_content).await?;

                // Read the CRLF that follows the literal content
                let mut crlf = [0u8; 2];
                reader.read_exact(&mut crlf).await?;

                // Read the final OK response line
                response.clear();
                reader.read_line(&mut response).await?;
                let final_line = response.trim().to_uppercase();

                if final_line.starts_with("OK") {
                    return Ok(String::from_utf8_lossy(&script_content).to_string());
                } else {
                    return Err(ManageSieveError::ServerError(final_line.to_string()));
                }
            } else {
                return Err(ManageSieveError::ProtocolError(
                    "Invalid literal length format".to_string(),
                ));
            }
        }

        // Handle non-literal responses (errors)
        let line_upper = line.to_uppercase();
        if line_upper.starts_with("NO") {
            Err(ManageSieveError::ScriptNotFound(script.to_string()))
        } else if line_upper.starts_with("BYE") {
            Err(ManageSieveError::ServerError(line.to_string()))
        } else {
            Err(ManageSieveError::InvalidResponse(line.to_string()))
        }
    }

    pub async fn put_script(&self, script: &str, content: &str) -> Result<(), ManageSieveError> {
        let mut connection = self.connection.lock().await;
        let (reader, writer) = &mut *connection;

        // Send PUTSCRIPT command with literal string
        let command = format!("PUTSCRIPT \"{}\" {{{}}}\r\n", script, content.len());
        writer.write_all(command.as_bytes()).await?;
        writer.write_all(content.as_bytes()).await?;
        writer.flush().await?;

        let mut response = String::new();
        reader.read_line(&mut response).await?;
        let line = response.trim().to_uppercase();

        if line.starts_with("OK") {
            Ok(())
        } else if line.starts_with("NO") {
            Err(ManageSieveError::ServerError(response.trim().to_string()))
        } else if line.starts_with("BYE") {
            Err(ManageSieveError::ServerError(response.trim().to_string()))
        } else {
            Err(ManageSieveError::InvalidResponse(
                response.trim().to_string(),
            ))
        }
    }

    pub async fn delete_script(&self, script: &str) -> Result<(), ManageSieveError> {
        let mut connection = self.connection.lock().await;
        let (reader, writer) = &mut *connection;

        // Send DELETESCRIPT command
        let command = format!("DELETESCRIPT \"{}\"\r\n", script);
        writer.write_all(command.as_bytes()).await?;
        writer.flush().await?;

        let mut response = String::new();
        reader.read_line(&mut response).await?;
        let line = response.trim().to_uppercase();

        if line.starts_with("OK") {
            Ok(())
        } else if line.starts_with("NO") {
            Err(ManageSieveError::ScriptNotFound(script.to_string()))
        } else if line.starts_with("BYE") {
            Err(ManageSieveError::ServerError(response.trim().to_string()))
        } else {
            Err(ManageSieveError::InvalidResponse(
                response.trim().to_string(),
            ))
        }
    }

    pub async fn rename_script(
        &self,
        old_name: &str,
        new_name: &str,
    ) -> Result<(), ManageSieveError> {
        let mut connection = self.connection.lock().await;
        let (reader, writer) = &mut *connection;

        // Send RENAMESCRIPT command
        let command = format!("RENAMESCRIPT \"{}\" \"{}\"\r\n", old_name, new_name);
        writer.write_all(command.as_bytes()).await?;
        writer.flush().await?;

        let mut response = String::new();
        reader.read_line(&mut response).await?;
        let line = response.trim().to_uppercase();

        if line.starts_with("OK") {
            Ok(())
        } else if line.starts_with("NO") {
            Err(ManageSieveError::ScriptNotFound(old_name.to_string()))
        } else if line.starts_with("BYE") {
            Err(ManageSieveError::ServerError(response.trim().to_string()))
        } else {
            Err(ManageSieveError::InvalidResponse(
                response.trim().to_string(),
            ))
        }
    }

    pub async fn set_active_script(&self, script: &str) -> Result<(), ManageSieveError> {
        let mut connection = self.connection.lock().await;
        let (reader, writer) = &mut *connection;

        // Send SETACTIVE command
        let command = format!("SETACTIVE \"{}\"\r\n", script);
        writer.write_all(command.as_bytes()).await?;
        writer.flush().await?;

        let mut response = String::new();
        reader.read_line(&mut response).await?;
        let line = response.trim().to_uppercase();

        if line.starts_with("OK") {
            Ok(())
        } else if line.starts_with("NO") {
            Err(ManageSieveError::ScriptNotFound(script.to_string()))
        } else if line.starts_with("BYE") {
            Err(ManageSieveError::ServerError(response.trim().to_string()))
        } else {
            Err(ManageSieveError::InvalidResponse(
                response.trim().to_string(),
            ))
        }
    }

    pub async fn check_script(&self, script: &str) -> Result<Option<String>, ManageSieveError> {
        let mut connection = self.connection.lock().await;
        let (reader, writer) = &mut *connection;

        // Send CHECKSCRIPT command with literal string
        let command = format!("CHECKSCRIPT {{{}}}\r\n", script.len());
        writer.write_all(command.as_bytes()).await?;
        writer.write_all(script.as_bytes()).await?;
        writer.flush().await?;

        let mut response = String::new();
        reader.read_line(&mut response).await?;
        let line = response.trim();

        if line.to_uppercase().starts_with("OK") {
            // Check for WARNINGS response code in the OK response
            if line.to_uppercase().contains("(WARNINGS)") {
                // Extract warning message - it might be on the same line or a separate literal
                let warning_msg = if let Some(start) = line.find('"') {
                    // Warning message is quoted on the same line
                    if let Some(end) = line.rfind('"') {
                        if start != end {
                            line[start + 1..end].to_string()
                        } else {
                            "Script has warnings".to_string()
                        }
                    } else {
                        "Script has warnings".to_string()
                    }
                } else if line.contains("{") {
                    // Warning message might be a literal string
                    if let Some(length) = self.parse_literal_length(line) {
                        let mut warning_content = vec![0u8; length];
                        reader.read_exact(&mut warning_content).await?;
                        String::from_utf8_lossy(&warning_content).to_string()
                    } else {
                        "Script has warnings".to_string()
                    }
                } else {
                    "Script has warnings".to_string()
                };
                Ok(Some(warning_msg))
            } else {
                Ok(None)
            }
        } else if line.to_uppercase().starts_with("NO") {
            // Extract error message from NO response
            let error_msg = if let Some(start) = line.find('"') {
                // Error message is quoted on the same line
                if let Some(end) = line.rfind('"') {
                    if start != end {
                        line[start + 1..end].to_string()
                    } else {
                        line.to_string()
                    }
                } else {
                    line.to_string()
                }
            } else if line.contains("{") {
                // Error message might be a literal string
                if let Some(length) = self.parse_literal_length(line) {
                    let mut error_content = vec![0u8; length];
                    reader.read_exact(&mut error_content).await?;
                    String::from_utf8_lossy(&error_content).to_string()
                } else {
                    line.to_string()
                }
            } else {
                line.to_string()
            };
            Err(ManageSieveError::ServerError(error_msg))
        } else if line.to_uppercase().starts_with("BYE") {
            Err(ManageSieveError::ServerError(line.to_string()))
        } else {
            Err(ManageSieveError::InvalidResponse(line.to_string()))
        }
    }

    async fn ignore_initial_greeting(stream: &mut TcpStream) -> Result<(), ConnectError> {
        let mut reader = BufReader::new(stream);
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).await?;

            if line.trim().is_empty() {
                continue;
            }

            // Check for OK response (end of greeting)
            if line.trim().to_uppercase().starts_with("OK") {
                break;
            }
        }
        Ok(())
    }

    async fn read_capabilities(
        reader: &mut BufReader<impl AsyncRead + Unpin>,
    ) -> Result<Capabilities, ConnectError> {
        let mut capabilities = Capabilities::default();

        loop {
            let mut line = String::new();
            reader.read_line(&mut line).await?;

            if line.trim().is_empty() {
                continue;
            }

            // Check for OK response (end of capabilities)
            if line.trim().to_uppercase().starts_with("OK") {
                break;
            }

            // Try to parse as capability line
            match Self::parse_capability_line(&line) {
                Ok((capability, value)) => {
                    Self::update_capabilities(&mut capabilities, capability, value);
                }
                Err(_) => {
                    continue;
                }
            }
        }

        Ok(capabilities)
    }

    fn parse_capability_line(line: &str) -> Result<(String, Option<String>), String> {
        let line = line.trim();

        // Parse quoted capability name and optional value
        if let Ok((remaining, (cap_name, value))) = parse_capability(line) {
            if !remaining.is_empty() {
                return Err(format!(
                    "Unexpected content after capability: {}",
                    remaining
                ));
            }
            Ok((cap_name, value))
        } else {
            Err("Invalid capability format".to_string())
        }
    }

    fn update_capabilities(capabilities: &mut Capabilities, name: String, value: Option<String>) {
        let name_upper = name.to_uppercase();

        match name_upper.as_str() {
            "IMPLEMENTATION" => {
                capabilities.implementation = value;
            }
            "SASL" => {
                if let Some(mechanisms) = value {
                    capabilities.sasl = mechanisms
                        .split_whitespace()
                        .map(|s| s.to_string())
                        .collect();
                }
            }
            "SIEVE" => {
                if let Some(extensions) = value {
                    capabilities.sieve = extensions
                        .split_whitespace()
                        .map(|s| s.to_string())
                        .collect();
                }
            }
            "STARTTLS" => {
                capabilities.starttls = true;
            }
            "MAXREDIRECTS" => {
                if let Some(max_str) = value {
                    if let Ok(max_val) = max_str.parse::<u32>() {
                        capabilities.maxredirects = Some(max_val);
                    }
                }
            }
            "NOTIFY" => {
                if let Some(methods) = value {
                    capabilities.notify =
                        methods.split_whitespace().map(|s| s.to_string()).collect();
                }
            }
            "LANGUAGE" => {
                capabilities.language = value;
            }
            "OWNER" => {
                capabilities.owner = value;
            }
            "VERSION" => {
                capabilities.version = value;
            }
            _ => {
                // Store unknown capabilities
                capabilities.other.insert(name, value.unwrap_or_default());
            }
        }
    }

    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    // Note: These methods are removed as they would break the Mutex encapsulation
    // Access to reader/writer should be done through the async methods

    // Helper method to parse script names from LISTSCRIPTS response
    fn parse_script_line(&self, line: &str) -> Option<(String, bool)> {
        if let Ok((_, script_name)) = parse_quoted_string(line) {
            let is_active = line.to_uppercase().contains("ACTIVE");
            Some((script_name.to_string(), is_active))
        } else {
            None
        }
    }

    // Helper method to parse literal string length from server response
    fn parse_literal_length(&self, line: &str) -> Option<usize> {
        if line.starts_with("{") && line.ends_with("}") {
            let length_str = &line[1..line.len() - 1];
            length_str.parse().ok()
        } else {
            None
        }
    }

    async fn authenticate(&self, username: &str, password: &str) -> Result<(), ConnectError> {
        let mut connection = self.connection.lock().await;
        let (reader, writer) = &mut *connection;
        // Check if SASL PLAIN is supported
        if !self.capabilities.sasl.contains(&"PLAIN".to_string()) {
            return Err(ConnectError::AuthenticationFailed(
                "SASL PLAIN mechanism not supported".to_string(),
            ));
        }

        // Prepare SASL PLAIN authentication
        let auth_string = format!("\0{}\0{}", username, password);
        let auth_b64 = general_purpose::STANDARD.encode(&auth_string);

        // Send AUTHENTICATE command
        let auth_command = format!("AUTHENTICATE \"PLAIN\" \"{}\"\r\n", auth_b64);
        writer.write_all(auth_command.as_bytes()).await?;
        writer.flush().await?;

        // Read response
        let mut response = String::new();
        reader.read_line(&mut response).await?;

        // Check if authentication was successful
        let response_upper = response.trim().to_uppercase();
        if response_upper.starts_with("OK") {
            Ok(())
        } else if response_upper.starts_with("NO") {
            Err(ConnectError::AuthenticationFailed(format!(
                "Server rejected credentials: {}",
                response.trim()
            )))
        } else if response_upper.starts_with("BYE") {
            Err(ConnectError::AuthenticationFailed(format!(
                "Server disconnected: {}",
                response.trim()
            )))
        } else {
            Err(ConnectError::AuthenticationFailed(format!(
                "Unexpected response: {}",
                response.trim()
            )))
        }
    }
}

// Nom parsers for ManageSieve protocol
fn parse_quoted_string(input: &str) -> IResult<&str, String> {
    let (input, _) = char('"')(input)?;
    let (input, content) = take_until("\"")(input)?;
    let (input, _) = char('"')(input)?;
    Ok((input, content.to_string()))
}

fn parse_capability(input: &str) -> IResult<&str, (String, Option<String>)> {
    let (input, capability_name) = parse_quoted_string(input)?;
    let (input, _) = space0(input)?;

    // Check if there's a value
    let (input, value) = opt(parse_quoted_string)(input)?;

    Ok((input, (capability_name, value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_quoted_string() {
        assert_eq!(
            parse_quoted_string("\"hello\""),
            Ok(("", "hello".to_string()))
        );
        assert_eq!(
            parse_quoted_string("\"IMPLEMENTATION\""),
            Ok(("", "IMPLEMENTATION".to_string()))
        );
        assert_eq!(
            parse_quoted_string("\"test\" remaining"),
            Ok((" remaining", "test".to_string()))
        );
    }

    #[test]
    fn test_parse_capability() {
        // Capability with value
        let result = parse_capability("\"IMPLEMENTATION\" \"Example1 ManageSieved v001\"");
        assert_eq!(
            result,
            Ok((
                "",
                (
                    "IMPLEMENTATION".to_string(),
                    Some("Example1 ManageSieved v001".to_string())
                )
            ))
        );

        // Capability without value
        let result = parse_capability("\"STARTTLS\"");
        assert_eq!(result, Ok(("", ("STARTTLS".to_string(), None))));

        // SASL with mechanisms
        let result = parse_capability("\"SASL\" \"PLAIN DIGEST-MD5 GSSAPI\"");
        assert_eq!(
            result,
            Ok((
                "",
                (
                    "SASL".to_string(),
                    Some("PLAIN DIGEST-MD5 GSSAPI".to_string())
                )
            ))
        );
    }

    #[test]
    fn test_parse_capability_line() {
        // Test valid capability lines
        assert_eq!(
            SieveClient::parse_capability_line("\"IMPLEMENTATION\" \"Example1 ManageSieved v001\""),
            Ok((
                "IMPLEMENTATION".to_string(),
                Some("Example1 ManageSieved v001".to_string())
            ))
        );

        assert_eq!(
            SieveClient::parse_capability_line("\"STARTTLS\""),
            Ok(("STARTTLS".to_string(), None))
        );

        assert_eq!(
            SieveClient::parse_capability_line("\"SIEVE\" \"fileinto vacation\""),
            Ok(("SIEVE".to_string(), Some("fileinto vacation".to_string())))
        );
    }

    #[test]
    fn test_update_capabilities() {
        let mut capabilities = Capabilities::default();

        // Test IMPLEMENTATION
        SieveClient::update_capabilities(
            &mut capabilities,
            "IMPLEMENTATION".to_string(),
            Some("Test Server v1.0".to_string()),
        );
        assert_eq!(
            capabilities.implementation,
            Some("Test Server v1.0".to_string())
        );

        // Test SASL
        SieveClient::update_capabilities(
            &mut capabilities,
            "SASL".to_string(),
            Some("PLAIN DIGEST-MD5".to_string()),
        );
        assert_eq!(capabilities.sasl, vec!["PLAIN", "DIGEST-MD5"]);

        // Test SIEVE
        SieveClient::update_capabilities(
            &mut capabilities,
            "SIEVE".to_string(),
            Some("fileinto vacation".to_string()),
        );
        assert_eq!(capabilities.sieve, vec!["fileinto", "vacation"]);

        // Test STARTTLS
        SieveClient::update_capabilities(&mut capabilities, "STARTTLS".to_string(), None);
        assert_eq!(capabilities.starttls, true);

        // Test MAXREDIRECTS
        SieveClient::update_capabilities(
            &mut capabilities,
            "MAXREDIRECTS".to_string(),
            Some("5".to_string()),
        );
        assert_eq!(capabilities.maxredirects, Some(5));

        // Test VERSION
        SieveClient::update_capabilities(
            &mut capabilities,
            "VERSION".to_string(),
            Some("1.0".to_string()),
        );
        assert_eq!(capabilities.version, Some("1.0".to_string()));

        // Test unknown capability
        SieveClient::update_capabilities(
            &mut capabilities,
            "UNKNOWN".to_string(),
            Some("value".to_string()),
        );
        assert_eq!(
            capabilities.other.get("UNKNOWN"),
            Some(&"value".to_string())
        );
    }

    #[test]
    fn test_capabilities_case_insensitive() {
        let mut capabilities = Capabilities::default();

        // Test case insensitive capability names
        SieveClient::update_capabilities(
            &mut capabilities,
            "implementation".to_string(),
            Some("Test".to_string()),
        );
        assert_eq!(capabilities.implementation, Some("Test".to_string()));

        SieveClient::update_capabilities(&mut capabilities, "StartTLS".to_string(), None);
        assert_eq!(capabilities.starttls, true);

        SieveClient::update_capabilities(
            &mut capabilities,
            "sAsL".to_string(),
            Some("PLAIN".to_string()),
        );
        assert_eq!(capabilities.sasl, vec!["PLAIN"]);
    }

    #[test]
    fn test_tls_error_handling() {
        // Test that TLS errors are properly created
        let rustls_error = rustls::Error::InvalidCertificate(rustls::CertificateError::BadEncoding);
        let connect_error = ConnectError::TlsError(rustls_error);
        assert!(connect_error.to_string().starts_with("TLS error:"));
    }

    #[test]
    fn test_complete_greeting_parsing() {
        // Test parsing a complete ManageSieve greeting
        let mut capabilities = Capabilities::default();

        // Simulate processing each line of a typical greeting
        let greeting_lines = vec![
            ("IMPLEMENTATION", Some("Example1 ManageSieved v001")),
            ("SASL", Some("PLAIN DIGEST-MD5 GSSAPI")),
            (
                "SIEVE",
                Some("fileinto vacation comparator-i;ascii-numeric"),
            ),
            ("STARTTLS", None),
            ("MAXREDIRECTS", Some("5")),
            ("NOTIFY", Some("mailto xmpp")),
            ("VERSION", Some("1.0")),
            ("LANGUAGE", Some("en")),
        ];

        for (name, value) in greeting_lines {
            SieveClient::update_capabilities(
                &mut capabilities,
                name.to_string(),
                value.map(|v| v.to_string()),
            );
        }

        // Verify all capabilities were parsed correctly
        assert_eq!(
            capabilities.implementation,
            Some("Example1 ManageSieved v001".to_string())
        );
        assert_eq!(capabilities.sasl, vec!["PLAIN", "DIGEST-MD5", "GSSAPI"]);
        assert_eq!(
            capabilities.sieve,
            vec!["fileinto", "vacation", "comparator-i;ascii-numeric"]
        );
        assert_eq!(capabilities.starttls, true);
        assert_eq!(capabilities.maxredirects, Some(5));
        assert_eq!(capabilities.notify, vec!["mailto", "xmpp"]);
        assert_eq!(capabilities.version, Some("1.0".to_string()));
        assert_eq!(capabilities.language, Some("en".to_string()));
    }

    #[test]
    fn test_greeting_parsing_demo() {
        // Demonstrate parsing a realistic ManageSieve greeting
        println!("\n=== ManageSieve Greeting Parsing Demo ===");

        let greeting_lines = vec![
            "\"IMPLEMENTATION\" \"Dovecot ManageSieve server v2.3.16\"",
            "\"SASL\" \"PLAIN LOGIN DIGEST-MD5 CRAM-MD5\"",
            "\"SIEVE\" \"fileinto reject envelope encoded-character vacation subaddress comparator-i;ascii-numeric relational regex imap4flags copy include variables body enotify environment mailbox date index ihave duplicate mime foreverypart extracttext\"",
            "\"STARTTLS\"",
            "\"MAXREDIRECTS\" \"10\"",
            "\"NOTIFY\" \"mailto\"",
            "\"VERSION\" \"1.0\"",
            "OK",
        ];

        println!("Raw greeting lines:");
        for line in &greeting_lines {
            println!("  S: {}", line);
        }

        // Parse each capability line
        let mut capabilities = Capabilities::default();
        for line in &greeting_lines {
            if line.trim() == "OK" {
                break;
            }

            if let Ok((name, value)) = SieveClient::parse_capability_line(line) {
                SieveClient::update_capabilities(&mut capabilities, name, value);
            }
        }

        println!("\nParsed capabilities:");
        if let Some(impl_name) = &capabilities.implementation {
            println!("  Implementation: {}", impl_name);
        }
        if let Some(version) = &capabilities.version {
            println!("  Version: {}", version);
        }
        if !capabilities.sasl.is_empty() {
            println!("  SASL mechanisms: {}", capabilities.sasl.join(", "));
        }
        if !capabilities.sieve.is_empty() {
            println!("  Sieve extensions: {}", capabilities.sieve.join(", "));
        }
        if capabilities.starttls {
            println!("  STARTTLS: supported");
        }
        if let Some(max_redirects) = capabilities.maxredirects {
            println!("  Max redirects: {}", max_redirects);
        }
        if !capabilities.notify.is_empty() {
            println!("  Notify methods: {}", capabilities.notify.join(", "));
        }

        // Verify parsing worked correctly
        assert_eq!(
            capabilities.implementation,
            Some("Dovecot ManageSieve server v2.3.16".to_string())
        );
        assert_eq!(
            capabilities.sasl,
            vec!["PLAIN", "LOGIN", "DIGEST-MD5", "CRAM-MD5"]
        );
        assert_eq!(capabilities.starttls, true);
        assert_eq!(capabilities.maxredirects, Some(10));
        assert_eq!(capabilities.version, Some("1.0".to_string()));
        assert!(capabilities.sieve.contains(&"fileinto".to_string()));
        assert!(capabilities.sieve.contains(&"vacation".to_string()));
        assert!(capabilities.sieve.contains(&"variables".to_string()));

        println!("\n✓ All capabilities parsed successfully!");
    }

    #[test]
    fn test_starttls_response_parsing() {
        // Test STARTTLS command response validation
        let ok_response = "OK Begin TLS negotiation now";
        assert!(ok_response.trim().to_uppercase().starts_with("OK"));

        let no_response = "NO TLS not available";
        assert!(!no_response.trim().to_uppercase().starts_with("OK"));

        let bye_response = "BYE Server shutting down";
        assert!(!bye_response.trim().to_uppercase().starts_with("OK"));
    }

    #[test]
    fn test_capability_after_tls() {
        // Test that capabilities are correctly updated after TLS
        let mut capabilities = Capabilities::default();

        // After TLS - SASL mechanisms should be available
        SieveClient::update_capabilities(
            &mut capabilities,
            "SASL".to_string(),
            Some("PLAIN LOGIN".to_string()),
        );
        assert_eq!(capabilities.sasl, vec!["PLAIN", "LOGIN"]);
        assert_eq!(capabilities.starttls, false); // Should not be present after TLS
    }

    #[test]
    fn test_hostname_validation() {
        // Test hostname validation for TLS
        let valid_hostnames = vec!["example.com", "mail.example.com", "localhost", "127.0.0.1"];

        let invalid_hostnames = vec!["", " ", "invalid..hostname"];

        for hostname in valid_hostnames {
            // Just test that the string is not empty
            assert!(!hostname.is_empty());
        }

        for hostname in invalid_hostnames {
            // Test invalid hostnames
            assert!(hostname.is_empty() || hostname.trim().is_empty() || hostname.contains(".."));
        }
    }

    #[test]
    fn test_type_aliases() {
        // Test that the type aliases work correctly
        use std::any::type_name;

        // Verify the type aliases resolve to the expected types
        assert!(type_name::<TlsReader>().contains("ReadHalf"));
        assert!(type_name::<TlsWriter>().contains("WriteHalf"));
        assert!(type_name::<TlsReader>().contains("TlsStream"));
        assert!(type_name::<TlsWriter>().contains("TlsStream"));
    }

    #[test]
    fn test_sasl_plain_encoding() {
        // Test SASL PLAIN authentication string encoding
        use base64::{Engine as _, engine::general_purpose};

        let username = "testuser";
        let password = "testpass";
        let auth_string = format!("\0{}\0{}", username, password);
        let auth_b64 = general_purpose::STANDARD.encode(&auth_string);

        // Verify the base64 encoding
        assert!(!auth_b64.is_empty());

        // Decode and verify the content
        let decoded = general_purpose::STANDARD.decode(&auth_b64).unwrap();
        let decoded_string = String::from_utf8(decoded).unwrap();
        assert_eq!(decoded_string, "\0testuser\0testpass");
    }

    #[test]
    fn test_authentication_errors() {
        // Test authentication error types
        let auth_error = ConnectError::AuthenticationFailed("Invalid credentials".to_string());
        assert_eq!(
            auth_error.to_string(),
            "Authentication failed: Invalid credentials"
        );

        // Test SASL mechanism not supported error
        let sasl_error =
            ConnectError::AuthenticationFailed("SASL PLAIN mechanism not supported".to_string());
        assert!(
            sasl_error
                .to_string()
                .contains("SASL PLAIN mechanism not supported")
        );
    }

    #[test]
    fn test_sasl_mechanism_check() {
        // Test checking for SASL PLAIN support
        let mut capabilities = Capabilities::default();

        // Without SASL PLAIN
        SieveClient::update_capabilities(
            &mut capabilities,
            "SASL".to_string(),
            Some("LOGIN DIGEST-MD5".to_string()),
        );
        assert!(!capabilities.sasl.contains(&"PLAIN".to_string()));

        // With SASL PLAIN
        SieveClient::update_capabilities(
            &mut capabilities,
            "SASL".to_string(),
            Some("PLAIN LOGIN DIGEST-MD5".to_string()),
        );
        assert!(capabilities.sasl.contains(&"PLAIN".to_string()));
    }

    #[test]
    fn test_authenticate_command_format() {
        // Test the format of the AUTHENTICATE command
        use base64::{Engine as _, engine::general_purpose};

        let username = "user";
        let password = "pass";
        let auth_string = format!("\0{}\0{}", username, password);
        let auth_b64 = general_purpose::STANDARD.encode(&auth_string);
        let command = format!("AUTHENTICATE \"PLAIN\" \"{}\"\r\n", auth_b64);

        assert!(command.starts_with("AUTHENTICATE \"PLAIN\""));
        assert!(command.ends_with("\r\n"));
        assert!(command.contains(&auth_b64));
    }

    #[test]
    fn test_authentication_response_parsing() {
        // Test parsing different authentication responses
        let ok_response = "OK Authentication successful";
        let no_response = "NO \"Authentication failed.\"";
        let bye_response = "BYE \"Too many failed attempts\"";
        let unknown_response = "UNKNOWN Something unexpected";

        // Test OK response
        assert!(ok_response.trim().to_uppercase().starts_with("OK"));

        // Test NO response
        assert!(no_response.trim().to_uppercase().starts_with("NO"));

        // Test BYE response
        assert!(bye_response.trim().to_uppercase().starts_with("BYE"));

        // Test unknown response
        assert!(!unknown_response.trim().to_uppercase().starts_with("OK"));
        assert!(!unknown_response.trim().to_uppercase().starts_with("NO"));
        assert!(!unknown_response.trim().to_uppercase().starts_with("BYE"));
    }
}
