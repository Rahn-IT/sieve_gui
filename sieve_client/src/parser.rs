// TODO

use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::{is_not, tag, take_while},
    character::streaming::char,
    combinator::{map, opt, value, verify},
    multi::{fold_many0, many0, separated_list0, separated_list1},
    sequence::{delimited, pair, preceded, tuple},
};

mod util;

use util::{multispace0, multispace1, parse_string, parse_string_array};

fn parse_require(input: &str) -> IResult<&str, Vec<String>> {
    delimited(
        pair(tag("require "), multispace0),
        parse_string_array,
        char(';'),
    )
    .parse(input)
}

#[derive(Debug, PartialEq)]
enum StringComparisonType {
    Is,
    Contains,
    Matches,
    Regex,
}

fn parse_string_comparison_type(input: &str) -> IResult<&str, StringComparisonType> {
    alt((
        tag(":is").map(|_| StringComparisonType::Is),
        tag(":contains").map(|_| StringComparisonType::Contains),
        tag(":matches").map(|_| StringComparisonType::Matches),
        tag(":regex").map(|_| StringComparisonType::Regex),
    ))
    .parse(input)
}

#[derive(Debug, PartialEq)]
struct StringCondition {
    comparison_type: StringComparisonType,
    source: String,
    value: String,
}

fn parse_string_condition(input: &str) -> IResult<&str, StringCondition> {
    let (rest, (comparison_type, header, value)) = tuple((
        parse_string_comparison_type,
        preceded(multispace1, parse_string),
        preceded(multispace1, parse_string),
    ))
    .parse(input)?;

    Ok((
        rest,
        StringCondition {
            comparison_type,
            source: header,
            value,
        },
    ))
}

fn parse_condition_list(input: &str) -> IResult<&str, Vec<Condition>> {
    delimited(
        preceded(char('('), multispace0),
        separated_list1(
            delimited(multispace0, char(','), multispace0),
            parse_condition,
        ),
        preceded(multispace0, char(')')),
    )
    .parse(input)
}

#[derive(Debug, PartialEq)]
enum Condition {
    Header(StringCondition),
    Address(StringCondition),
    AllOf(Vec<Condition>),
    AnyOf(Vec<Condition>),
}

fn parse_condition(input: &str) -> IResult<&str, Condition> {
    alt((
        preceded(tag("header"), preceded(multispace1, parse_string_condition))
            .map(Condition::Header),
        preceded(
            tag("address"),
            preceded(multispace1, parse_string_condition),
        )
        .map(Condition::Address),
        preceded(tag("allof"), preceded(multispace0, parse_condition_list)).map(Condition::AllOf),
        preceded(tag("anyof"), preceded(multispace0, parse_condition_list)).map(Condition::AnyOf),
    ))
    .parse(input)
}

fn simple_if<'a>(
    if_type: &str,
) -> impl FnMut(&'a str) -> IResult<&'a str, (Condition, Vec<Expression>)> {
    pair(
        preceded(
            tag(if_type),
            delimited(multispace1, parse_condition, multispace1),
        ),
        delimited(
            char('{'),
            parse_expression_list,
            preceded(multispace0, char('}')),
        ),
    )
}

#[derive(Debug, PartialEq)]
struct If {
    condition: Condition,
    expressions: Vec<Expression>,
    else_ifs: Vec<(Condition, Vec<Expression>)>,
    else_block: Vec<Expression>,
}

fn parse_if(input: &str) -> IResult<&str, If> {
    let (rest, (condition, expressions)) = simple_if("if")(input)?;

    let (rest, else_ifs): (&str, Vec<(Condition, Vec<Expression>)>) =
        many0(preceded(multispace0, simple_if("elsif"))).parse(rest)?;
    let (rest, else_block) = opt(preceded(
        delimited(multispace0, tag("else"), multispace0),
        delimited(
            char('{'),
            parse_expression_list,
            preceded(multispace0, char('}')),
        ),
    ))
    .map(Option::unwrap_or_default)
    .parse(rest)?;

    Ok((
        rest,
        If {
            condition,
            expressions,
            else_ifs,
            else_block,
        },
    ))
}

#[derive(Debug, PartialEq)]
enum Flag {
    Seen,
    Flagged,
    Answered,
    Deleted,
    Draft,
    Recent,
    Custom(String),
}

fn parse_flags(input: &str) -> IResult<&str, Vec<Flag>> {
    let (rest, raw_flags) =
        alt((parse_string.map(|s| vec![s]), parse_string_array)).parse(input)?;

    let flags = raw_flags
        .into_iter()
        .map(|flag| match flag.as_str() {
            "\\Seen" => Flag::Seen,
            "\\Flagged" => Flag::Flagged,
            "\\Answered" => Flag::Answered,
            "\\Deleted" => Flag::Deleted,
            "\\Draft" => Flag::Draft,
            "\\Recent" => Flag::Recent,
            // Todo: only allow valid custom flags
            _ => Flag::Custom(flag),
        })
        .collect();

    Ok((rest, flags))
}

fn flag_command<'a>(command: &str) -> impl FnMut(&'a str) -> IResult<&'a str, Vec<Flag>> {
    delimited(
        tag(command),
        delimited(multispace1, parse_flags, multispace0),
        char(';'),
    )
}

#[derive(Debug, PartialEq)]
enum Expression {
    Require(Vec<String>),
    If(If),
    FileInto(String),
    AddFlag(Vec<Flag>),
    RemoveFlag(Vec<Flag>),
    SetFlag(Vec<Flag>),
    Discard,
    Keep,
    Stop,
}

fn parse_expression(input: &str) -> IResult<&str, Expression> {
    preceded(
        multispace0,
        alt((
            parse_require.map(Expression::Require),
            parse_if.map(Expression::If),
            flag_command("addflag").map(Expression::AddFlag),
            flag_command("removeflag").map(Expression::RemoveFlag),
            flag_command("setflag").map(Expression::SetFlag),
            tag("discard;").map(|_| Expression::Discard),
            tag("keep;").map(|_| Expression::Keep),
            tag("stop;").map(|_| Expression::Stop),
            delimited(
                tag("fileinto"),
                preceded(multispace1, parse_string),
                char(';'),
            )
            .map(Expression::FileInto),
        )),
    )
    .parse(input)
}

fn parse_expression_list(input: &str) -> IResult<&str, Vec<Expression>> {
    nom::multi::many0(parse_expression).parse(input)
}

#[cfg(test)]
mod test {
    use std::vec;

    use super::*;

    #[test]
    fn test_multispace() {
        assert_eq!(multispace0(r#""#), Ok(("", "")));
        assert_eq!(multispace0(" \n\t"), Ok(("", " \n\t")));
    }

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

    #[test]
    fn test_string_comparison_type() {
        assert_eq!(
            parse_string_comparison_type(r#":contains"#),
            Ok(("", StringComparisonType::Contains))
        );
        assert_eq!(
            parse_string_comparison_type(r#":matches"#),
            Ok(("", StringComparisonType::Matches))
        );
        assert_eq!(
            parse_string_comparison_type(r#":is"#),
            Ok(("", StringComparisonType::Is))
        );
    }

    #[test]
    fn test_string_comparison() {
        assert_eq!(
            parse_string_condition(r#":contains "Subject" "urgent""#),
            Ok((
                "",
                StringCondition {
                    comparison_type: StringComparisonType::Contains,
                    source: "Subject".to_string(),
                    value: "urgent".to_string()
                }
            ))
        )
    }

    #[test]
    fn test_condition() {
        assert_eq!(
            parse_condition(r#"header :contains "Subject" "urgent""#),
            Ok((
                "",
                Condition::Header(StringCondition {
                    comparison_type: StringComparisonType::Contains,
                    source: "Subject".to_string(),
                    value: "urgent".to_string()
                })
            ))
        );
    }

    #[test]
    fn test_flag() {
        assert_eq!(
            parse_flags(r#""Muffin""#),
            Ok(("", vec![Flag::Custom("Muffin".to_string())]))
        );
        assert_eq!(
            parse_flags(r#"["\\Seen", "Muffin"]"#),
            Ok(("", vec![Flag::Seen, Flag::Custom("Muffin".to_string())]))
        );
    }

    #[test]
    fn test_if() {
        assert_eq!(
            parse_if(r#"if header :contains "Subject" "urgent" { keep; }"#),
            Ok((
                "",
                If {
                    condition: Condition::Header(StringCondition {
                        comparison_type: StringComparisonType::Contains,
                        source: "Subject".to_string(),
                        value: "urgent".to_string()
                    }),
                    expressions: vec![Expression::Keep],
                    else_ifs: vec![],
                    else_block: vec![],
                }
            ))
        );
        assert_eq!(
            parse_if(r#"if header :contains "Subject" "urgent" { discard; }"#),
            Ok((
                "",
                If {
                    condition: Condition::Header(StringCondition {
                        comparison_type: StringComparisonType::Contains,
                        source: "Subject".to_string(),
                        value: "urgent".to_string()
                    }),
                    expressions: vec![Expression::Discard],
                    else_ifs: vec![],
                    else_block: vec![],
                }
            ))
        );
        assert_eq!(
            parse_if(r#"if header :contains "Subject" "urgent" { fileinto "urgent"; keep; }"#),
            Ok((
                "",
                If {
                    condition: Condition::Header(StringCondition {
                        comparison_type: StringComparisonType::Contains,
                        source: "Subject".to_string(),
                        value: "urgent".to_string()
                    }),
                    expressions: vec![Expression::FileInto("urgent".to_string()), Expression::Keep],
                    else_ifs: vec![],
                    else_block: vec![],
                }
            ))
        );
        assert_eq!(
            parse_if(
                r#"if header :contains "Subject" "urgent" { fileinto "urgent"; addflag "\\Flagged"; keep; } elsif header :contains "Subject" "cookies" { fileinto "cookies"; keep; } elsif header :contains "Subject" "muffins" { addflag ["\\Flagged", "Muffin"]; } else { discard; }"#
            ),
            Ok((
                "",
                If {
                    condition: Condition::Header(StringCondition {
                        comparison_type: StringComparisonType::Contains,
                        source: "Subject".to_string(),
                        value: "urgent".to_string()
                    }),
                    expressions: vec![
                        Expression::FileInto("urgent".to_string()),
                        Expression::AddFlag(vec![Flag::Flagged]),
                        Expression::Keep
                    ],
                    else_ifs: vec![
                        (
                            Condition::Header(StringCondition {
                                comparison_type: StringComparisonType::Contains,
                                source: "Subject".to_string(),
                                value: "cookies".to_string()
                            }),
                            vec![
                                Expression::FileInto("cookies".to_string()),
                                Expression::Keep
                            ]
                        ),
                        (
                            Condition::Header(StringCondition {
                                comparison_type: StringComparisonType::Contains,
                                source: "Subject".to_string(),
                                value: "muffins".to_string()
                            }),
                            vec![Expression::AddFlag(vec![
                                Flag::Flagged,
                                Flag::Custom("Muffin".to_string())
                            ])]
                        )
                    ],
                    else_block: vec![Expression::Discard],
                }
            ))
        );
    }

    #[test]
    fn parse_script() {
        assert_eq!(
            parse_expression_list(
                r#"
            require ["fileinto", "envelope", "regex", "relational", "comparator-i;ascii-numeric", "date", "environment"];

            if header :matches "Subject" "*urgent*" {
                fileinto "Urgent";
            }

            elsif header :regex "Subject" "\\[TICKET-[0-9]{4}\\]" {
                fileinto "Tickets";
            }

            elsif header :contains "Subject" "important" {
                addflag "\\Flagged";
            }

            else {
                discard;
            }
            "#
            ).unwrap().1,

                vec![
                    Expression::Require(vec![
                        String::from("fileinto"),
                        String::from("envelope"),
                        String::from("regex"),
                        String::from("relational"),
                        String::from("comparator-i;ascii-numeric"),
                        String::from("date"),
                        String::from("environment")
                    ]),
                    Expression::If(If {
                        condition: Condition::Header(StringCondition {
                            comparison_type: StringComparisonType::Matches,
                            source: "Subject".to_string(),
                            value: "*urgent*".to_string()
                        }),
                        expressions: vec![Expression::FileInto("Urgent".to_string())],
                        else_ifs: vec![
                            (
                                Condition::Header(StringCondition {
                                    comparison_type: StringComparisonType::Regex,
                                    source: "Subject".to_string(),
                                    value: "\\[TICKET-[0-9]{4}\\]".to_string()
                                }),
                                vec![Expression::FileInto("Tickets".to_string())]
                            ),
                            (
                                Condition::Header(StringCondition {
                                    comparison_type: StringComparisonType::Contains,
                                    source: "Subject".to_string(),
                                    value: "important".to_string()
                                }),
                                vec![Expression::AddFlag(vec![Flag::Flagged])]
                            ),
                        ],
                        else_block: vec![Expression::Discard],
                    })
                ]
        );

        assert_eq!(
            parse_expression_list(
                r#"
            require ["imap4flags","fileinto"];

            if allof (header :contains "subject" "backup successful") {

                addflag "\\Seen";

                fileinto "INBOX/Proxmox Backup";

            }

            if allof (address :contains "from" "ServiceQueue-noreply@teamviewer.com") {

                addflag "\\Seen";

                fileinto "INBOX/Teamviewer";

            }"#,
            ),
            Ok((
                "",
                vec![
                    Expression::Require(vec!["imap4flags".to_string(), "fileinto".to_string()]),
                    Expression::If(If {
                        condition: Condition::AllOf(vec![Condition::Header(StringCondition {
                            comparison_type: StringComparisonType::Contains,
                            source: "subject".to_string(),
                            value: "backup successful".to_string()
                        })]),
                        expressions: vec![
                            Expression::AddFlag(vec![Flag::Seen]),
                            Expression::FileInto("INBOX/Proxmox Backup".to_string())
                        ],
                        else_ifs: vec![],
                        else_block: vec![]
                    }),
                    Expression::If(If {
                        condition: Condition::AllOf(vec![Condition::Address(StringCondition {
                            comparison_type: StringComparisonType::Contains,
                            source: "from".to_string(),
                            value: "ServiceQueue-noreply@teamviewer.com".to_string()
                        })]),
                        expressions: vec![
                            Expression::AddFlag(vec![Flag::Seen]),
                            Expression::FileInto("INBOX/Teamviewer".to_string())
                        ],
                        else_ifs: vec![],
                        else_block: vec![]
                    })
                ]
            ))
        );

        assert_eq!(parse_expression_list(r#"
            require ["imap4flags","fileinto","body"];

            if allof (header :contains "subject" "ALTERNATE - Rechnung") {

                addflag "\\Seen";

                addflag "berechnen";

                fileinto "INBOX/Belege/Alternate";

            }

            if allof (address :contains "from" "info@elovade.com") {

                addflag "\\Seen";

                addflag "berechnen";

                fileinto "INBOX/Belege/Elovade";

            }

            if allof (header :contains "subject" "FinHelper Rechnung") {

                addflag "\\Seen";

                fileinto "INBOX/Belege/Finhelper";

            }

            if allof (header :contains "subject" "Ihre IONOS Rechnung") {

                addflag "\\Seen";

                fileinto "INBOX/Belege/Ionos";

            }

            if allof (header :contains "subject" "Jakobsoftware - Ihre Rechnung") {

                addflag "\\Seen";

                addflag "berechnen";

            }

            if allof (header :contains "subject" "Rechnungskopie", address :contains "from" "info@servercow.de") {

                addflag "\\Seen";

                fileinto "INBOX/Belege/Mailcow - Servercow";

                addflag "berechnen";

            }

            if allof (address :contains "from" "shop@office-partner.de", header :contains "subject" "Bestellinformation und Rechnung") {

                addflag "\\Seen";

                fileinto "INBOX/Belege/OfficePartner";

                addflag "berechnen";

            }

            if allof (address :contains "from" "team@sipgate.de", header :contains "subject" "Rechnung") {

                addflag "\\Seen";

                fileinto "INBOX/Belege/Sipgrate";

            }

            if allof (address :contains "from" "no-reply-member@afterbuy.de") {

                addflag "\\Seen";

                fileinto "INBOX/Belege/Softwarebilliger";

            }

            if allof (header :contains "subject" "Ihre STRATO-Rechnung") {

                addflag "\\Seen";

            }

            if allof (address :contains "from" "rechnungonline@telekom.de") {

                fileinto "INBOX/Belege/Telekom";

                addflag "\\Seen";

            }

            if allof (header :contains "subject" "backup successful") {

                addflag "\\Seen";

                fileinto "INBOX/Meldungen/Proxmox Backup";

            }

            if allof (address :contains "from" "pbs@it-rahn.de", header :contains "subject" "successful") {

                addflag "\\Seen";

                fileinto "INBOX/Meldungen/PBS";

                stop;

            }

            if anyof (header :contains "subject" "Sync remote 'rahnit-pbs' datastore 'rahnit' successful", header :contains "subject" "Verify Datastore 'rahnit' successful") {

                addflag "\\Seen";

                fileinto "INBOX/Meldungen/PBS2";

            }

            if anyof (header :contains "subject" "Tagesbackup Erfolgreich", header :contains "subject" "Tagessicherung Neu Erfolgreich", header :contains "subject" "Tagessicherung intern Erfolgreich", header :contains "subject" "Tagesbackup neu Erfolgreich", header :contains "subject" "Cloud-Sicherung Erfolgreich", header :contains "subject" "Wochensicherung Cloud : Erfolgreich", header :contains "subject" "Wiederherstellungstest Erfolgreich", header :contains "subject" "Wochensicherung Neu Erfolgreich", header :contains "subject" "Wochensicherung neue Version Erfolgreich", header :contains "subject" "Wochensicherung 2 Erfolgreich", header :contains "subject" "Tagessicherung : Erfolgreich", header :contains "subject" "Wochensicherung : Erfolgreich", header :contains "subject" "Sicherung Intern H Montag Dienstag Freitag Erfolgreich", header :contains "subject" "Sicherung Intern F Dienstag Donnerstag Erfolgreich") {

                addflag "\\Seen";

                fileinto "INBOX/Meldungen/Backupassist";

            }

            if allof (header :contains "subject" "Ihre Downloadinformation von softwarebilliger.de") {

                addflag "berechnen";

                fileinto "INBOX/Lizenzen";

            }

            if allof (address :contains "from" "noreply@3cx.net") {

                addflag "\\Seen";

                fileinto "INBOX/Meldungen/3CX";

            }

            if anyof (header :contains "subject" "ALTERNATE – Bestellung erhalten", header :contains "subject" "ALTERNATE – Bestellung im System eingegangen") {

                addflag "\\Seen";

                fileinto "INBOX/Werbung/Alternate";

            }

            if allof (header :contains "subject" "Keine Verbindung zum Gerät für 14+ Tage") {

                addflag "\\Seen";

                fileinto "INBOX/Meldungen/Avast";

            }

            if allof (header :contains "subject" "Ihre Bestellung", header :contains "subject" "bei OFFICE Partner") {

                addflag "\\Seen";

                fileinto "INBOX/Werbung/OfficePartner";

            }

            if anyof (header :contains "subject" "Proxmox Status Report", header :contains "subject" "Backup successful to pbs") {

                addflag "\\Seen";

                fileinto "INBOX/Meldungen/PMG";

            }

            if anyof (address :contains "from" "no-reply@notifications.ui.com", address :contains "from" "Unifi OS", header :contains "subject" "Threat Detected", header :contains "From" "no-reply@notifications.ui.com") {

                addflag "\\Seen";

                fileinto "INBOX/Meldungen/Unifi";

            }

            if allof (address :contains "from" "sales@allnet.de") {

                addflag "\\Seen";

                fileinto "INBOX/Werbung/Allnet";

            }

            if allof (address :contains "from" "mailings@mailings.gmx.net") {

                addflag "\\Seen";

                fileinto "Trash";

            }

            if anyof (header :contains "subject" "Neue Aufträge in Ihrem Tätigkeitsbereich") {

                addflag "\\Seen";

                fileinto "Trash";

            }

            if allof (header :contains "subject" "Ihre Konkurrenten schalten Google Ads") {

                addflag "\\Seen";

                fileinto "Trash";

            }

            if allof (header :contains "subject" "Ihre Bestellung im Softwarebilliger.de") {

                addflag "\\Seen";

                fileinto "INBOX/Werbung/Softwarebilliger";

            }"#).unwrap().0.len(), 0);
    }
}
