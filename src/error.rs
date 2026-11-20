// Hound -- A wav encoding and decoding library in Rust
// Copyright (C) 2015 Ruud van Asseldonk
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The error type for operations on `WavReader` and `WavWriter`.

use std::error;
use std::fmt;
use std::io;
use std::result;

/// The error type for operations on `WavReader` and `WavWriter`.
#[derive(Debug)]
pub enum Error {
    /// An IO error occured in the underlying reader or writer.
    IoError(io::Error),
    /// Ill-formed WAVE data was encountered.
    FormatError(&'static str),
    /// The sample has more bits than the destination type.
    ///
    /// When iterating using the `samples` iterator, this means that the
    /// destination type (produced by the iterator) is not wide enough to hold
    /// the sample. When writing, this means that the sample cannot be written,
    /// because it requires more bits than the bits per sample specified.
    TooWide,
    /// The number of samples written is not a multiple of the number of channels.
    UnfinishedSample,
    /// The format is not supported.
    Unsupported,
    /// The sample format is different than the destination format.
    ///
    /// When iterating using the `samples` iterator, this means the destination
    /// type (produced by the iterator) has a different sample format than the
    /// samples in the wav file.
    ///
    /// For example, this will occur if the user attempts to produce `i32`
    /// samples (which have a `SampleFormat::Int`) from a wav file that
    /// contains floating point data (`SampleFormat::Float`).
    InvalidSampleFormat,
    /// More data was written than the WAVE format can represent.
    ///
    /// The classic WAVE format stores the lengths of the file and of the data
    /// chunk as 32-bit integers, so a regular wav file can hold at most 4 GiB
    /// of audio data. This error is returned when more data is written and the
    /// writer was not created with `WavWriter::new_rf64`, which lifts the
    /// limit by writing an RF64 file instead.
    TooMuchData,
    /// An RF64 file does not contain a `ds64` chunk where one is required.
    ///
    /// The RF64 format requires the `ds64` chunk (which stores the true 64-bit
    /// lengths) to be the first chunk after the `RF64`/`WAVE` header. Without
    /// it, the actual lengths of the file and of the data chunk are unknown.
    MissingDs64,
    /// The `ds64` chunk of an RF64 file contradicts the `data` chunk header.
    ///
    /// In an RF64 file, the 32-bit length field of the `data` chunk must be
    /// either `0xffffffff` (which means: see the `ds64` chunk for the true
    /// length) or equal to the 64-bit data length stored in the `ds64` chunk.
    /// Any other value is inconsistent.
    InconsistentDs64,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> result::Result<(), fmt::Error> {
        match *self {
            Error::IoError(ref err) => err.fmt(formatter),
            Error::FormatError(reason) => {
                try!(formatter.write_str("Ill-formed WAVE file: "));
                formatter.write_str(reason)
            }
            Error::TooWide => {
                formatter.write_str("The sample has more bits than the destination type.")
            }
            Error::UnfinishedSample => {
                formatter.write_str(
                    "The number of samples written is not a multiple of the number of channels.")
            }
            Error::Unsupported => {
                formatter.write_str("The wave format of the file is not supported.")
            }
            Error::InvalidSampleFormat => {
                formatter.write_str("The sample format differs from the destination format.")
            }
            Error::TooMuchData => {
                formatter.write_str(
                    "The data exceeds the 4 GiB limit of the WAVE format; \
                     use WavWriter::new_rf64 to write an RF64 file instead.")
            }
            Error::MissingDs64 => {
                formatter.write_str(
                    "Ill-formed WAVE file: RF64 file without a ds64 chunk.")
            }
            Error::InconsistentDs64 => {
                formatter.write_str(
                    "Ill-formed WAVE file: the ds64 chunk contradicts the data chunk header.")
            }
        }
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        match *self {
            Error::IoError(ref err) => err.description(),
            Error::FormatError(reason) => reason,
            Error::TooWide => "the sample has more bits than the destination type",
            Error::UnfinishedSample => "the number of samples written is not a multiple of the number of channels",
            Error::Unsupported => "the wave format of the file is not supported",
            Error::InvalidSampleFormat => "the sample format differs from the destination format",
            Error::TooMuchData => "the data exceeds the 4 GiB limit of the WAVE format",
            Error::MissingDs64 => "RF64 file without a ds64 chunk",
            Error::InconsistentDs64 => "the ds64 chunk contradicts the data chunk header",
        }
    }

    fn cause(&self) -> Option<&error::Error> {
        match *self {
            Error::IoError(ref err) => Some(err),
            Error::FormatError(_) => None,
            Error::TooWide => None,
            Error::UnfinishedSample => None,
            Error::Unsupported => None,
            Error::InvalidSampleFormat => None,
            Error::TooMuchData => None,
            Error::MissingDs64 => None,
            Error::InconsistentDs64 => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::IoError(err)
    }
}

/// A type for results generated by Hound where the error type is hard-wired.
pub type Result<T> = result::Result<T, Error>;
