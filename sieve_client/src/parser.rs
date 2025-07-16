// TODO

use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::{is_not, tag, take_while},
    character::streaming::char,
    combinator::{map, opt, value, verify},
    multi::{fold_many0, many0, separated_list0},
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

fn multispace0(input: &str) -> IResult<&str, &str> {
    take_while(|c| match c {
        ' ' => true,
        '\t' => true,
        '\n' => true,
        '\r' => true,
        _ => false,
    })
    .parse(input)
}

fn multispace1(input: &str) -> IResult<&str, &str> {
    verify(multispace0, |s: &str| !s.is_empty()).parse(input)
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

#[derive(Debug, PartialEq)]
enum Condition {
    Header(StringCondition),
    Address(StringCondition),
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
    }
}
