// TODO

use nom::{
    IResult, Parser,
    branch::alt,
    bytes::streaming::{is_not, tag},
    character::streaming::{char, multispace0},
    combinator::{map, value, verify},
    multi::{fold_many0, separated_list0},
    sequence::{delimited, pair, preceded, tuple},
};

fn parse_escaped_char(input: &str) -> IResult<&str, char> {
    preceded(
        char('\\'),
        alt((value('\\', char('\\')), value('"', char('"')))),
    )
    .parse(input)
}

fn parse_unescaped_sequence(input: &str) -> IResult<&str, &str> {
    let not_quoted = is_not("\\\"");

    verify(not_quoted, |s: &str| !s.is_empty()).parse(input)
}

enum StringPart<'a> {
    Literal(&'a str),
    Escaped(char),
}

fn parse_string_part(input: &str) -> IResult<&str, StringPart> {
    alt((
        map(parse_unescaped_sequence, StringPart::Literal),
        map(parse_escaped_char, StringPart::Escaped),
    ))
    .parse(input)
}

fn parse_string(input: &str) -> IResult<&str, String> {
    let build_string = fold_many0(parse_string_part, String::new, |mut string, fragment| {
        match fragment {
            StringPart::Literal(literal) => string.push_str(literal),
            StringPart::Escaped(char) => string.push(char),
        };

        string
    });

    delimited(char('"'), build_string, char('"')).parse(input)
}

fn parse_string_array(input: &str) -> IResult<&str, Vec<String>> {
    delimited(
        pair(char('['), multispace0),
        separated_list0(tuple((multispace0, char(','), multispace0)), parse_string),
        pair(multispace0, char(']')),
    )
    .parse(input)
}

fn parse_require(input: &str) -> IResult<&str, Vec<String>> {
    delimited(
        pair(tag("require "), multispace0),
        parse_string_array,
        char(';'),
    )
    .parse(input)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_string() {
        assert_eq!(parse_string(r#""hello""#), Ok(("", "hello".to_string())));
        assert_eq!(parse_string(r#""world""#), Ok(("", "world".to_string())));
        assert_eq!(
            parse_string(r#""hello, world""#),
            Ok(("", "hello, world".to_string()))
        );
        assert_eq!(
            parse_string(r#""hello\\world""#),
            Ok(("", "hello\\world".to_string()))
        );
        assert_eq!(
            parse_string(r#""hello\"world""#),
            Ok(("", "hello\"world".to_string()))
        );
    }

    #[test]
    fn test_parse_string_array() {
        assert_eq!(
            parse_string_array(r#"["hello","world"]"#),
            Ok(("", vec!["hello".to_string(), "world".to_string()]))
        );
        assert_eq!(
            parse_string_array(r#"[ "hello" , "world" ]"#),
            Ok(("", vec!["hello".to_string(), "world".to_string()]))
        );
        assert_eq!(
            parse_string_array(r#"["hello","world","hello, world"]"#),
            Ok((
                "",
                vec![
                    "hello".to_string(),
                    "world".to_string(),
                    "hello, world".to_string()
                ]
            ))
        );
        assert_eq!(
            parse_string_array(r#"["hello", "world", "hello\\world"]"#),
            Ok((
                "",
                vec![
                    "hello".to_string(),
                    "world".to_string(),
                    "hello\\world".to_string()
                ]
            ))
        );
        assert_eq!(
            parse_string_array(r#"["hello", "world", "hello\"world"]"#),
            Ok((
                "",
                vec![
                    "hello".to_string(),
                    "world".to_string(),
                    "hello\"world".to_string()
                ]
            ))
        );
    }

    #[test]
    fn test_parse_require() {
        assert_eq!(
            parse_require(r#"require ["fileinto", "vacation"];"#),
            Ok(("", vec!["fileinto".to_string(), "vacation".to_string()]))
        );
    }
}
