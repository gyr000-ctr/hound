//! Acceptance tests for streaming wav I/O with unknown length.
//!
//! A `WavWriter` created with `WavWriter::new_stream` does not require the
//! underlying writer to be seekable. It writes a header with the
//! "unknown length" signal value `0xFFFFFFFF`, and a `WavReader` delimits
//! such a data chunk by the end of the stream.

use hound::{SampleFormat, WavError, WavReader, WavSpec, WavWriter};
use std::io::{self, Cursor, Write};

/// A write-only sink that is deliberately not seekable, to model recording
/// to a pipe, a socket, or stdout.
struct Sink {
    buf: Vec<u8>,
}

impl Sink {
    fn new() -> Sink {
        Sink { buf: Vec::new() }
    }
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn spec_mono_16() -> WavSpec {
    WavSpec {
        channels: 1,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    }
}

fn spec_stereo_16() -> WavSpec {
    WavSpec {
        channels: 2,
        ..spec_mono_16()
    }
}

/// A stream writer writes the "unknown length" signal value `0xFFFFFFFF`
/// for both the RIFF chunk size and the data chunk length, and dropping it
/// does not attempt to seek (which a pipe would not survive semantically).
#[test]
fn stream_unknown_len_header_uses_signal_value() {
    let mut sink = Sink::new();
    {
        let mut writer = WavWriter::new_stream(&mut sink, spec_mono_16()).unwrap();
        writer.write_sample(7_i16).unwrap();
        writer.write_sample(-3_i16).unwrap();
        // The writer keeps a running count of the samples written.
        assert_eq!(writer.len(), 2);
        assert_eq!(writer.duration(), 2);
    }

    let bytes = &sink.buf;
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[4..8], &[0xff, 0xff, 0xff, 0xff]);
    assert_eq!(&bytes[8..12], b"WAVE");
    assert_eq!(&bytes[36..40], b"data");
    assert_eq!(&bytes[40..44], &[0xff, 0xff, 0xff, 0xff]);
    // The samples follow the 44-byte header, little endian.
    assert_eq!(&bytes[44..], &[7, 0, 0xfd, 0xff]);
}

/// Samples written to a non-seekable stream can be read back: the reader
/// interprets `0xFFFFFFFF` as "delimited by the end of the stream".
#[test]
fn stream_unknown_len_reads_until_eof() {
    let samples: Vec<i16> = (-1024..1024).collect();

    let mut sink = Sink::new();
    {
        let mut writer = WavWriter::new_stream(&mut sink, spec_mono_16()).unwrap();
        for &s in &samples {
            writer.write_sample(s).unwrap();
        }
        // Flushing a stream works without seeking.
        writer.flush().unwrap();
    }

    let mut reader = WavReader::new(Cursor::new(&sink.buf)).unwrap();
    assert_eq!(reader.spec(), spec_mono_16());
    // The length is not known up front for an EOF-delimited stream.
    assert_eq!(reader.len(), std::u32::MAX);

    let read: Vec<i16> = reader.samples().map(|r| r.unwrap()).collect();
    assert_eq!(read, samples);

    // After the end of the stream the iterator stays exhausted.
    assert!(reader.samples::<i16>().next().is_none());
}

/// The header produced by `WavSpec::into_header_for_infinite_file` is
/// readable with the same EOF-delimited semantics.
#[test]
fn stream_unknown_len_reads_infinite_header_from_spec() {
    let spec = spec_mono_16();
    let mut bytes = spec.into_header_for_infinite_file();
    bytes.extend_from_slice(&5_i16.to_le_bytes());
    bytes.extend_from_slice(&(-7_i16).to_le_bytes());

    let mut reader = WavReader::new(Cursor::new(&bytes)).unwrap();
    assert_eq!(reader.spec(), spec);
    let read: Vec<i16> = reader.samples().map(|r| r.unwrap()).collect();
    assert_eq!(read, vec![5, -7]);
}

/// `finish()` back-patches the sample counts into the header when the
/// underlying writer is seekable.
#[test]
fn stream_unknown_len_finish_back_patches_counts_when_seekable() {
    let samples: Vec<i16> = (0..100).map(|x| x * 7 - 300).collect();

    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = WavWriter::new(&mut cursor, spec_mono_16()).unwrap();
        for &s in &samples {
            writer.write_sample(s).unwrap();
        }
        writer.finish().unwrap();
    }
    let bytes = cursor.into_inner();

    // finish() back-patches the RIFF chunk size and the data chunk length.
    let data_len = (samples.len() * 2) as u32;
    assert_eq!(&bytes[4..8], &(36 + data_len).to_le_bytes());
    assert_eq!(&bytes[40..44], &data_len.to_le_bytes());

    // The result is an ordinary, fully specified wav file.
    let mut reader = WavReader::new(Cursor::new(&bytes)).unwrap();
    assert_eq!(reader.len(), samples.len() as u32);
    let read: Vec<i16> = reader.samples().map(|r| r.unwrap()).collect();
    assert_eq!(read, samples);
}

/// The seekable path is byte-for-byte unchanged: `finish()` produces
/// exactly the same bytes as `finalize()`.
#[test]
fn stream_unknown_len_finish_matches_finalize_bytes() {
    let samples: Vec<i16> = (-50..50).collect();

    let mut finalized = Cursor::new(Vec::new());
    {
        let mut writer = WavWriter::new(&mut finalized, spec_stereo_16()).unwrap();
        for &s in &samples {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
    }

    let mut finished = Cursor::new(Vec::new());
    {
        let mut writer = WavWriter::new(&mut finished, spec_stereo_16()).unwrap();
        for &s in &samples {
            writer.write_sample(s).unwrap();
        }
        writer.finish().unwrap();
    }

    assert_eq!(finalized.into_inner(), finished.into_inner());
}

/// The seekable path must keep producing the exact same header bytes as
/// before streaming support was added.
#[test]
fn stream_unknown_len_seekable_header_is_unchanged() {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = WavWriter::new(&mut cursor, spec_mono_16()).unwrap();
        writer.write_sample(0x0102_i16).unwrap();
        writer.write_sample(0x0304_i16).unwrap();
        writer.finalize().unwrap();
    }

    let expected: Vec<u8> = vec![
        0x52, 0x49, 0x46, 0x46, // "RIFF"
        40, 0, 0, 0, //            RIFF chunk size: 36 + 4 data bytes
        0x57, 0x41, 0x56, 0x45, // "WAVE"
        0x66, 0x6d, 0x74, 0x20, // "fmt "
        16, 0, 0, 0, //            fmt chunk size
        1, 0, //                   WAVE_FORMAT_PCM
        1, 0, //                   1 channel
        0x44, 0xac, 0, 0, //       44100 samples per second
        0x88, 0x58, 0x01, 0, //    88200 bytes per second
        2, 0, //                   block align
        16, 0, //                  bits per sample
        0x64, 0x61, 0x74, 0x61, // "data"
        4, 0, 0, 0, //             data chunk size
        0x02, 0x01, 0x04, 0x03, // two samples, little endian
    ];
    assert_eq!(cursor.into_inner(), expected);
}

/// `finish()` on a non-seekable stream cannot back-patch the counts and
/// fails with `WavError::CannotFinalize`.
#[test]
fn stream_unknown_len_finish_on_non_seekable_stream_fails() {
    let mut sink = Sink::new();
    let mut writer = WavWriter::new_stream(&mut sink, spec_mono_16()).unwrap();
    writer.write_sample(1_i16).unwrap();
    match writer.finish() {
        Err(WavError::CannotFinalize) => {}
        other => panic!("expected CannotFinalize, got {:?}", other),
    }
}

/// A writer created with `new_stream` never captured a way to seek, so it
/// cannot be finalized even when the underlying writer happens to be
/// seekable. (To back-patch counts, create the writer with `new`.)
#[test]
fn stream_unknown_len_finalize_on_stream_writer_fails() {
    let mut cursor = Cursor::new(Vec::new());
    let mut writer = WavWriter::new_stream(&mut cursor, spec_mono_16()).unwrap();
    writer.write_sample(1_i16).unwrap();
    match writer.finalize() {
        Err(WavError::CannotFinalize) => {}
        other => panic!("expected CannotFinalize, got {:?}", other),
    }
}

/// A stream that ends in the middle of a sample yields
/// `WavError::StreamTruncated`, distinct from a clean end of stream.
#[test]
fn stream_unknown_len_truncated_mid_sample_signals_error() {
    let mut sink = Sink::new();
    {
        let mut writer = WavWriter::new_stream(&mut sink, spec_mono_16()).unwrap();
        writer.write_sample(0x1234_i16).unwrap();
    }
    // Truncate the stream in the middle of the second sample.
    sink.buf.push(0xaa);

    let mut reader = WavReader::new(Cursor::new(&sink.buf)).unwrap();
    assert_eq!(reader.samples::<i16>().next().unwrap().unwrap(), 0x1234);
    match reader.samples::<i16>().next() {
        Some(Err(WavError::StreamTruncated)) => {}
        other => panic!("expected StreamTruncated, got {:?}", other),
    }
    // After the truncation error the iterator is exhausted.
    assert!(reader.samples::<i16>().next().is_none());
}

/// A stream that ends with a number of samples that is not a multiple of
/// the channel count was truncated mid-frame.
#[test]
fn stream_unknown_len_truncated_mid_frame_signals_error() {
    let mut sink = Sink::new();
    {
        let mut writer = WavWriter::new_stream(&mut sink, spec_stereo_16()).unwrap();
        writer.write_sample(1_i16).unwrap();
        writer.write_sample(2_i16).unwrap();
        // The right channel of the second frame never arrives.
        writer.write_sample(3_i16).unwrap();
    }

    let mut reader = WavReader::new(Cursor::new(&sink.buf)).unwrap();
    let mut samples = reader.samples::<i16>();
    assert_eq!(samples.next().unwrap().unwrap(), 1);
    assert_eq!(samples.next().unwrap().unwrap(), 2);
    assert_eq!(samples.next().unwrap().unwrap(), 3);
    match samples.next() {
        Some(Err(WavError::StreamTruncated)) => {}
        other => panic!("expected StreamTruncated, got {:?}", other),
    }
    assert!(samples.next().is_none());
}

/// `WavError` is the public name of the error type; the new variants are
/// reachable through both names.
#[test]
fn stream_unknown_len_wav_error_aliases_error() {
    let _: WavError = hound::Error::StreamTruncated;
    let _: hound::Error = WavError::CannotFinalize;
}
