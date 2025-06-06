// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.
// spell-checker:ignore (ToDO) bigdecimal extendedbigdecimal numberparse hexadecimalfloat
use std::ffi::OsString;
use std::io::{stdout, ErrorKind, Write};

use clap::{Arg, ArgAction, Command};
use num_traits::{ToPrimitive, Zero};

use uucore::error::{FromIo, UResult};
use uucore::format::{num_format, sprintf, Format, FormatArgument};
use uucore::{format_usage, help_about, help_usage};

mod error;
mod extendedbigdecimal;
mod hexadecimalfloat;

// public to allow fuzzing
#[cfg(fuzzing)]
pub mod number;
#[cfg(not(fuzzing))]
mod number;
mod numberparse;
use crate::error::SeqError;
use crate::extendedbigdecimal::ExtendedBigDecimal;
use crate::number::PreciseNumber;

const ABOUT: &str = help_about!("seq.md");
const USAGE: &str = help_usage!("seq.md");

const OPT_SEPARATOR: &str = "separator";
const OPT_TERMINATOR: &str = "terminator";
const OPT_EQUAL_WIDTH: &str = "equal-width";
const OPT_FORMAT: &str = "format";

const ARG_NUMBERS: &str = "numbers";

#[derive(Clone)]
struct SeqOptions<'a> {
    separator: String,
    terminator: String,
    equal_width: bool,
    format: Option<&'a str>,
}

/// A range of floats.
///
/// The elements are (first, increment, last).
type RangeFloat = (ExtendedBigDecimal, ExtendedBigDecimal, ExtendedBigDecimal);

// Turn short args with attached value, for example "-s,", into two args "-s" and "," to make
// them work with clap.
fn split_short_args_with_value(args: impl uucore::Args) -> impl uucore::Args {
    let mut v: Vec<OsString> = Vec::new();

    for arg in args {
        let bytes = arg.as_encoded_bytes();

        if bytes.len() > 2
            && (bytes.starts_with(b"-f") || bytes.starts_with(b"-s") || bytes.starts_with(b"-t"))
        {
            let (short_arg, value) = bytes.split_at(2);
            // SAFETY:
            // Both `short_arg` and `value` only contain content that originated from `OsStr::as_encoded_bytes`
            v.push(unsafe { OsString::from_encoded_bytes_unchecked(short_arg.to_vec()) });
            v.push(unsafe { OsString::from_encoded_bytes_unchecked(value.to_vec()) });
        } else {
            v.push(arg);
        }
    }

    v.into_iter()
}

fn select_precision(
    first: Option<usize>,
    increment: Option<usize>,
    last: Option<usize>,
) -> Option<usize> {
    match (first, increment, last) {
        (Some(0), Some(0), Some(0)) => Some(0),
        (Some(f), Some(i), Some(_)) => Some(f.max(i)),
        _ => None,
    }
}

#[uucore::main]
pub fn uumain(args: impl uucore::Args) -> UResult<()> {
    let matches = uu_app().try_get_matches_from(split_short_args_with_value(args))?;

    let numbers_option = matches.get_many::<String>(ARG_NUMBERS);

    if numbers_option.is_none() {
        return Err(SeqError::NoArguments.into());
    }

    let numbers = numbers_option.unwrap().collect::<Vec<_>>();

    let options = SeqOptions {
        separator: matches
            .get_one::<String>(OPT_SEPARATOR)
            .map_or("\n", |s| s.as_str())
            .to_string(),
        terminator: matches
            .get_one::<String>(OPT_TERMINATOR)
            .map(|s| s.as_str())
            .unwrap_or("\n")
            .to_string(),
        equal_width: matches.get_flag(OPT_EQUAL_WIDTH),
        format: matches.get_one::<String>(OPT_FORMAT).map(|s| s.as_str()),
    };

    let (first, first_precision) = if numbers.len() > 1 {
        match numbers[0].parse() {
            Ok(num) => (num, hexadecimalfloat::parse_precision(numbers[0])),
            Err(e) => return Err(SeqError::ParseError(numbers[0].to_string(), e).into()),
        }
    } else {
        (PreciseNumber::one(), Some(0))
    };
    let (increment, increment_precision) = if numbers.len() > 2 {
        match numbers[1].parse() {
            Ok(num) => (num, hexadecimalfloat::parse_precision(numbers[1])),
            Err(e) => return Err(SeqError::ParseError(numbers[1].to_string(), e).into()),
        }
    } else {
        (PreciseNumber::one(), Some(0))
    };
    if increment.is_zero() {
        return Err(SeqError::ZeroIncrement(numbers[1].to_string()).into());
    }
    let (last, last_precision): (PreciseNumber, Option<usize>) = {
        // We are guaranteed that `numbers.len()` is greater than zero
        // and at most three because of the argument specification in
        // `uu_app()`.
        let n: usize = numbers.len();
        match numbers[n - 1].parse() {
            Ok(num) => (num, hexadecimalfloat::parse_precision(numbers[n - 1])),
            Err(e) => return Err(SeqError::ParseError(numbers[n - 1].to_string(), e).into()),
        }
    };

    let padding = first
        .num_integral_digits
        .max(increment.num_integral_digits)
        .max(last.num_integral_digits);

    let precision = select_precision(first_precision, increment_precision, last_precision);

    let format = options
        .format
        .map(Format::<num_format::Float>::parse)
        .transpose()?;

    let result = print_seq(
        (first.number, increment.number, last.number),
        precision,
        &options.separator,
        &options.terminator,
        options.equal_width,
        padding,
        format.as_ref(),
    );
    match result {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::BrokenPipe => Ok(()),
        Err(err) => Err(err.map_err_context(|| "write error".into())),
    }
}

pub fn uu_app() -> Command {
    Command::new(uucore::util_name())
        .trailing_var_arg(true)
        .infer_long_args(true)
        .version(uucore::crate_version!())
        .about(ABOUT)
        .override_usage(format_usage(USAGE))
        .arg(
            Arg::new(OPT_SEPARATOR)
                .short('s')
                .long("separator")
                .help("Separator character (defaults to \\n)"),
        )
        .arg(
            Arg::new(OPT_TERMINATOR)
                .short('t')
                .long("terminator")
                .help("Terminator character (defaults to \\n)"),
        )
        .arg(
            Arg::new(OPT_EQUAL_WIDTH)
                .short('w')
                .long("equal-width")
                .help("Equalize widths of all numbers by padding with zeros")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new(OPT_FORMAT)
                .short('f')
                .long(OPT_FORMAT)
                .help("use printf style floating-point FORMAT"),
        )
        .arg(
            // we use allow_hyphen_values instead of allow_negative_numbers because clap removed
            // the support for "exotic" negative numbers like -.1 (see https://github.com/clap-rs/clap/discussions/5837)
            Arg::new(ARG_NUMBERS)
                .allow_hyphen_values(true)
                .action(ArgAction::Append)
                .num_args(1..=3),
        )
}

fn done_printing<T: Zero + PartialOrd>(next: &T, increment: &T, last: &T) -> bool {
    if increment >= &T::zero() {
        next > last
    } else {
        next < last
    }
}

fn format_bigdecimal(value: &bigdecimal::BigDecimal) -> Option<String> {
    let format_arguments = &[FormatArgument::Float(value.to_f64()?)];
    let value_as_bytes = sprintf("%g", format_arguments).ok()?;
    String::from_utf8(value_as_bytes).ok()
}

/// Write a big decimal formatted according to the given parameters.
fn write_value_float(
    writer: &mut impl Write,
    value: &ExtendedBigDecimal,
    width: usize,
    precision: Option<usize>,
) -> std::io::Result<()> {
    let value_as_str = match precision {
        // format with precision: decimal floats and integers
        Some(precision) => match value {
            ExtendedBigDecimal::Infinity | ExtendedBigDecimal::MinusInfinity => {
                format!("{value:>width$.precision$}")
            }
            _ => format!("{value:>0width$.precision$}"),
        },
        // format without precision: hexadecimal floats
        None => match value {
            ExtendedBigDecimal::BigDecimal(bd) => {
                format_bigdecimal(bd).unwrap_or_else(|| "{value}".to_owned())
            }
            _ => format!("{value:>0width$}"),
        },
    };
    write!(writer, "{value_as_str}")
}

/// Floating point based code path
fn print_seq(
    range: RangeFloat,
    precision: Option<usize>,
    separator: &str,
    terminator: &str,
    pad: bool,
    padding: usize,
    format: Option<&Format<num_format::Float>>,
) -> std::io::Result<()> {
    let stdout = stdout();
    let mut stdout = stdout.lock();
    let (first, increment, last) = range;
    let mut value = first;
    let padding = if pad {
        let precision_value = precision.unwrap_or(0);
        padding
            + if precision_value > 0 {
                precision_value + 1
            } else {
                0
            }
    } else {
        0
    };
    let mut is_first_iteration = true;
    while !done_printing(&value, &increment, &last) {
        if !is_first_iteration {
            write!(stdout, "{separator}")?;
        }
        // If there was an argument `-f FORMAT`, then use that format
        // template instead of the default formatting strategy.
        //
        // TODO The `printf()` method takes a string as its second
        // parameter but we have an `ExtendedBigDecimal`. In order to
        // satisfy the signature of the function, we convert the
        // `ExtendedBigDecimal` into a string. The `printf()`
        // logic will subsequently parse that string into something
        // similar to an `ExtendedBigDecimal` again before rendering
        // it as a string and ultimately writing to `stdout`. We
        // shouldn't have to do so much converting back and forth via
        // strings.
        match &format {
            Some(f) => {
                let float = match &value {
                    ExtendedBigDecimal::BigDecimal(bd) => bd.to_f64().unwrap(),
                    ExtendedBigDecimal::Infinity => f64::INFINITY,
                    ExtendedBigDecimal::MinusInfinity => f64::NEG_INFINITY,
                    ExtendedBigDecimal::MinusZero => -0.0,
                    ExtendedBigDecimal::Nan => f64::NAN,
                };
                f.fmt(&mut stdout, float)?;
            }
            None => write_value_float(&mut stdout, &value, padding, precision)?,
        }
        // TODO Implement augmenting addition.
        value = value + increment.clone();
        is_first_iteration = false;
    }
    if !is_first_iteration {
        write!(stdout, "{terminator}")?;
    }
    stdout.flush()?;
    Ok(())
}
