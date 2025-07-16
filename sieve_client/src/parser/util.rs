use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::{is_not, tag, take_while},
    character::streaming::char,
    combinator::{map, opt, value, verify},
    multi::{fold_many0, many0, separated_list0, separated_list1},
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

pub fn parse_string(input: &str) -> IResult<&str, String> {
    let build_string = fold_many0(parse_string_part, String::new, |mut string, fragment| {
        match fragment {
            StringPart::Literal(literal) => string.push_str(literal),
            StringPart::Escaped(char) => string.push(char),
        };

        string
    });

    delimited(char('"'), build_string, char('"')).parse(input)
}

pub fn multispace0(input: &str) -> IResult<&str, &str> {
    take_while(|c| match c {
        ' ' => true,
        '\t' => true,
        '\n' => true,
        '\r' => true,
        _ => false,
    })
    .parse(input)
}

pub fn multispace1(input: &str) -> IResult<&str, &str> {
    verify(multispace0, |s: &str| !s.is_empty()).parse(input)
}

pub fn parse_string_array(input: &str) -> IResult<&str, Vec<String>> {
    delimited(
        pair(char('['), multispace0),
        separated_list0(delimited(multispace0, char(','), multispace0), parse_string),
        pair(multispace0, char(']')),
    )
    .parse(input)
}
