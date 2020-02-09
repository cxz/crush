use crate::commands::CompileContext;
use crate::{
    data::{
        Argument,
        Row,
        ValueType,
        Value,
    },
    stream::{OutputStream},
    errors::{JobError, argument_error},
};
use std::{
    io::BufReader,
    io::prelude::*,
};

extern crate map_in_place;

use map_in_place::MapVecInPlace;
use crate::printer::Printer;
use crate::data::{ColumnType, BinaryReader};
use crate::errors::JobResult;
use crate::stream::ValueReceiver;

pub struct Config {
    separator: char,
    columns: Vec<ColumnType>,
    skip_head: usize,
    trim: Option<char>,
    input: Box<dyn BinaryReader>,
}

fn parse(arguments: Vec<Argument>, input: ValueReceiver) -> JobResult<Config> {
    let mut separator = ',';
    let mut columns = Vec::new();
    let mut skip_head = 0;
    let mut trim = None;
    let mut files = Vec::new();

    for arg in arguments {
        match &arg.name {
            None => {
                arg.value.file_expand(&mut files);
            }
            Some(name) => {
                match (name.as_ref(), arg.value) {
                    ("col", Value::Text(s)) => {
                        let split: Vec<&str> = s.split(':').collect();
                        match split.len() {
                            2 => columns.push(ColumnType::named(split[0], ValueType::from(split[1])?)),
                            _ => return Err(argument_error(format!("Expected a column description on the form name:type, got {}", s).as_str())),
                        }
                    }

                    ("head", Value::Integer(s)) => skip_head = s as usize,

                    ("sep", Value::Text(s)) => {
                        if s.len() == 1 {
                            separator = s.chars().next().unwrap();
                        } else {
                            return Err(argument_error("Separator must be exactly one character long"));
                        }
                    }

                    ("trim", Value::Text(s)) => {
                        if s.len() == 1 {
                            trim = Some(s.chars().next().unwrap());
                        } else {
                            return Err(argument_error("Only one character can be trimmed"));
                        }
                    }

                    _ => return Err(argument_error(format!("Unknown parameter {}", name).as_str())),
                }
            }
        }
    }

    let reader = match files.len() {
            0 => {
                let v = input.recv()?;
                match v {
                    Value::BinaryReader(b) => {
                        Ok(b)
                    }
                    _ => Err(argument_error("Expected either a file to read or binary pipe input"))
                }
            }
            1 => {
                BinaryReader::from(&files[0])
            }
            _ => Err(argument_error("Expected a file name"))
        }?;

    Ok(Config {
        separator,
        columns,
        skip_head,
        trim,
        input: reader,
    })
}

fn run(cfg: Config, output: OutputStream, printer: Printer) -> JobResult<()> {

    let printer_copy = printer.clone();

    let separator = cfg.separator.clone();
    let trim = cfg.trim.clone();
    let columns = cfg.columns.clone();
    let skip = cfg.skip_head;

    let mut reader = BufReader::new(cfg.input.reader());
    let mut line = String::new();
    let mut skipped = 0usize;
    loop {
        line.clear();
        reader.read_line(&mut line);
        if line.is_empty() {
            break;
        }
        if skipped < skip {
            skipped += 1;
            continue;
        }
        let line_without_newline = &line[0..line.len() - 1];
        let mut split: Vec<&str> = line_without_newline
            .split(separator)
            .map(|s| trim
                .map(|c| s.trim_matches(c))
                .unwrap_or(s))
            .collect();
        if split.len() != columns.len() {
            printer_copy.error("csv: Wrong number of columns in CSV file");
        }
        if let Some(trim) = trim {
            split = split.map(|s| s.trim_matches(trim));
        }

        match split.iter()
            .zip(columns.iter())
            .map({ |(s, t)| t.cell_type.parse(*s) })
            .collect::<Result<Vec<Value>, JobError>>() {
            Ok(cells) => { output.send(Row::new(cells)); }
            Err(err) => { printer_copy.job_error(err); }
        }
    }
    return Ok(());
}

pub fn perform(context: CompileContext) -> JobResult<()> {
    let cfg = parse(context.arguments, context.input)?;
    let output = context.output.initialize(
        cfg.columns.clone())?;
    run(cfg, output, context.printer)
}
