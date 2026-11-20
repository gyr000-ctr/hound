// Acceptance tests for RF64 support in hound.
//
// Run with: cargo test --manifest-path tests/accept/Cargo.toml rf64 -q

extern crate hound;

use hound::{Error, SampleFormat, WavError, WavReader, WavSpec, WavWriter};
use std::cmp;
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};

fn spec16() -> WavSpec {
    WavSpec {
        channels: 1,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    }
}

fn read_u32_le(bytes: &[u8]) -> u32 {
    let mut buf = [0u8; 4];
    buf.copy_from_slice(bytes);
    u32::from_le_bytes(buf)
}

fn read_u64_le(bytes: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(bytes);
    u64::from_le_bytes(buf)
}

fn append_fmt_chunk(v: &mut Vec<u8>) {
    v.extend_from_slice(b"fmt ");
    v.extend_from_slice(&16_u32.to_le_bytes()); // fmt chunk size
    v.extend_from_slice(&1_u16.to_le_bytes()); // WAVE_FORMAT_PCM
    v.extend_from_slice(&1_u16.to_le_bytes()); // channels
    v.extend_from_slice(&44100_u32.to_le_bytes()); // sample rate
    v.extend_from_slice(&88200_u32.to_le_bytes()); // bytes per second
    v.extend_from_slice(&2_u16.to_le_bytes()); // block align
    v.extend_from_slice(&16_u16.to_le_bytes()); // bits per sample
}

/// Builds the 44-byte classic RIFF header for mono 16-bit 44.1 kHz PCM,
/// with the given value in the data chunk length field.
fn classic_header(data_len: u32) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&0xff_ff_ff_ff_u32.to_le_bytes());
    v.extend_from_slice(b"WAVE");
    append_fmt_chunk(&mut v);
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    v
}

/// Builds an RF64 file with a ds64 chunk that claims `ds64_data_len` bytes of
/// data, a data chunk whose 32-bit length field holds `data_field`, followed
/// by `data` itself.
fn rf64_bytes(ds64_data_len: u64, data_field: u32, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"RF64");
    v.extend_from_slice(&0xff_ff_ff_ff_u32.to_le_bytes());
    v.extend_from_slice(b"WAVE");
    v.extend_from_slice(b"ds64");
    v.extend_from_slice(&28_u32.to_le_bytes()); // ds64 chunk size
    v.extend_from_slice(&0_u64.to_le_bytes()); // riff size (not validated)
    v.extend_from_slice(&ds64_data_len.to_le_bytes());
    v.extend_from_slice(&0_u64.to_le_bytes()); // sample count
    v.extend_from_slice(&0_u32.to_le_bytes()); // table length
    append_fmt_chunk(&mut v);
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_field.to_le_bytes());
    v.extend_from_slice(data);
    v
}

/// A writer that discards all data but records small writes at low offsets,
/// so that header (ds64) updates can be inspected afterwards. Memory use is
/// bounded no matter how much data is written.
struct SinkWriter {
    pos: u64,
    /// The largest position ever written to; the total file size.
    high_water: u64,
    recorded: Vec<(u64, Vec<u8>)>,
}

impl SinkWriter {
    fn new() -> SinkWriter {
        SinkWriter {
            pos: 0,
            high_water: 0,
            recorded: Vec::new(),
        }
    }

    /// Returns the bytes of the last write that started at `offset`.
    fn last_write_at(&self, offset: u64) -> &[u8] {
        for &(off, ref bytes) in self.recorded.iter().rev() {
            if off == offset {
                return bytes;
            }
        }
        panic!("no recorded write at offset {}", offset);
    }
}

impl io::Write for SinkWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.pos < 4096 {
            self.recorded.push((self.pos, buf.to_vec()));
        }
        self.pos += buf.len() as u64;
        self.high_water = cmp::max(self.high_water, self.pos);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl io::Seek for SinkWriter {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.pos = match pos {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(n) => (self.pos as i64 + n) as u64,
            SeekFrom::End(n) => n as u64,
        };
        Ok(self.pos)
    }
}

/// A read/write/seek in-memory file of unbounded size: bytes beyond the
/// initial header read as zeroes, and writes are discarded. Used to simulate
/// a nearly-full 4 GiB file without using 4 GiB of memory.
struct SparseFile {
    pos: u64,
    header: Vec<u8>,
}

impl io::Read for SparseFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let start = self.pos as usize;
        if start < self.header.len() {
            let n = cmp::min(buf.len(), self.header.len() - start);
            buf[..n].copy_from_slice(&self.header[start..start + n]);
            self.pos += n as u64;
            Ok(n)
        } else {
            for b in buf.iter_mut() {
                *b = 0;
            }
            self.pos += buf.len() as u64;
            Ok(buf.len())
        }
    }
}

impl io::Write for SparseFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pos += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl io::Seek for SparseFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.pos = match pos {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(n) => (self.pos as i64 + n) as u64,
            SeekFrom::End(n) => n as u64,
        };
        Ok(self.pos)
    }
}

#[test]
fn rf64_roundtrip_small_file() {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut writer = WavWriter::new_rf64(&mut buf, spec16()).unwrap();
        for s in 0..1000_i16 {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
    }

    let bytes = buf.get_ref();
    let total_len = bytes.len() as u64;

    // The file starts with the RF64 magic and 0xffffffff size placeholders.
    assert_eq!(&bytes[0..4], b"RF64");
    assert_eq!(&bytes[4..8], &[0xff; 4]);
    assert_eq!(&bytes[8..12], b"WAVE");

    // The ds64 chunk is the first chunk and holds the true 64-bit lengths.
    assert_eq!(&bytes[12..16], b"ds64");
    assert_eq!(read_u32_le(&bytes[16..20]), 28);
    assert_eq!(read_u64_le(&bytes[20..28]), total_len - 8); // riff size
    assert_eq!(read_u64_le(&bytes[28..36]), 2000); // data size
    assert_eq!(read_u64_le(&bytes[36..44]), 1000); // sample count (frames)
    assert_eq!(read_u32_le(&bytes[44..48]), 0); // table length

    // The fmt chunk follows, then the data chunk with a placeholder length.
    assert_eq!(&bytes[48..52], b"fmt ");
    assert_eq!(&bytes[72..76], b"data");
    assert_eq!(&bytes[76..80], &[0xff; 4]);
    assert_eq!(total_len, 80 + 2000);

    // The reader recognizes the RF64 file automatically.
    buf.set_position(0);
    let mut reader = WavReader::new(&mut buf).unwrap();
    assert_eq!(reader.spec(), spec16());
    assert_eq!(reader.len(), 1000);
    assert_eq!(reader.duration(), 1000);
    let samples: Vec<i16> = reader.samples().map(|s| s.unwrap()).collect();
    assert_eq!(samples, (0..1000_i16).collect::<Vec<i16>>());
}

#[test]
fn rf64_write_beyond_4gib_uses_constant_memory() {
    // Write more than 4 GiB of 16-bit mono samples to a sink that stores
    // nothing but a few header writes, so peak memory stays O(1).
    const CHUNK: u32 = 1 << 22; // samples per flush, an 8 MiB buffer
    const CHUNKS: u64 = 513; // 513 * 2^22 samples = 4 GiB + 8 MiB of data
    let total_samples = CHUNK as u64 * CHUNKS;
    let total_data = total_samples * 2;

    let mut sink = SinkWriter::new();
    {
        let mut writer = WavWriter::new_rf64(&mut sink, spec16()).unwrap();
        for _ in 0..CHUNKS {
            let mut sample_writer = writer.get_i16_writer(CHUNK);
            for _ in 0..CHUNK {
                unsafe { sample_writer.write_sample_unchecked(0_i16) };
            }
            sample_writer.flush().unwrap();
        }
        assert!(total_data > std::u32::MAX as u64);
        assert_eq!(writer.len(), total_samples as u32);
        writer.finalize().unwrap();
    }

    // The header of an RF64 file with a PCMWAVEFORMAT fmt chunk is 80 bytes.
    assert_eq!(sink.high_water, 80 + total_data);

    // The ds64 chunk was updated with the true 64-bit lengths on finalize.
    assert_eq!(read_u64_le(sink.last_write_at(20)), 80 + total_data - 8);
    assert_eq!(read_u64_le(sink.last_write_at(28)), total_data);
    assert_eq!(read_u64_le(sink.last_write_at(36)), total_samples);

    // The file started with the RF64 magic.
    assert_eq!(&sink.last_write_at(0)[..4], b"RF64");
}

#[test]
fn rf64_too_much_data_only_without_rf64() {
    // Simulate a classic RIFF file whose data chunk is already almost 4 GiB,
    // and append to it. One more sample would overflow the 32-bit length
    // field, which must fail with TooMuchData.
    let file = SparseFile {
        pos: 0,
        header: classic_header(0xff_ff_ff_fe),
    };
    let mut writer = WavWriter::new_append(file).unwrap();

    match writer.write_sample(0_i16) {
        Err(Error::TooMuchData) => {}
        other => panic!("expected TooMuchData, got {:?}", other),
    }
    // The error is also reachable through the WavError alias, and the failed
    // write did not advance the data counter, so writing fails again.
    match writer.write_sample(0_i16) {
        Err(WavError::TooMuchData) => {}
        other => panic!("expected TooMuchData, got {:?}", other),
    }

    // A writer that does have room left does not report TooMuchData.
    let file = SparseFile {
        pos: 0,
        header: classic_header(0xff_ff_ff_fc),
    };
    let mut writer = WavWriter::new_append(file).unwrap();
    writer.write_sample(0_i16).unwrap(); // 0xfffffffe bytes, still fits
    match writer.write_sample(0_i16) {
        Err(Error::TooMuchData) => {}
        other => panic!("expected TooMuchData, got {:?}", other),
    }
}

#[test]
fn rf64_missing_ds64_is_rejected() {
    // An RF64 file without a ds64 chunk must be rejected, and the error must
    // be distinguishable from a contradictory ds64 chunk.
    let mut v = Vec::new();
    v.extend_from_slice(b"RF64");
    v.extend_from_slice(&0xff_ff_ff_ff_u32.to_le_bytes());
    v.extend_from_slice(b"WAVE");
    append_fmt_chunk(&mut v);
    v.extend_from_slice(b"data");
    v.extend_from_slice(&8_u32.to_le_bytes());
    v.extend_from_slice(&[0u8; 8]);

    // The 12-byte header itself is recognized as RF64.
    assert!(hound::read_wave_header(&mut &v[..]).is_ok());

    match WavReader::new(Cursor::new(v)) {
        Err(Error::MissingDs64) => {}
        other => panic!("expected MissingDs64, got {:?}", other.map(|_| ())),
    }

    // A ds64 chunk that is not the first chunk is just as missing.
    let mut v = Vec::new();
    v.extend_from_slice(b"RF64");
    v.extend_from_slice(&0xff_ff_ff_ff_u32.to_le_bytes());
    v.extend_from_slice(b"WAVE");
    append_fmt_chunk(&mut v);
    v.extend_from_slice(b"ds64");
    v.extend_from_slice(&28_u32.to_le_bytes());
    v.extend_from_slice(&0_u64.to_le_bytes());
    v.extend_from_slice(&8_u64.to_le_bytes());
    v.extend_from_slice(&4_u64.to_le_bytes());
    v.extend_from_slice(&0_u32.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&0xff_ff_ff_ff_u32.to_le_bytes());
    v.extend_from_slice(&[0u8; 8]);

    match WavReader::new(Cursor::new(v)) {
        Err(Error::MissingDs64) => {}
        other => panic!("expected MissingDs64, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn rf64_inconsistent_ds64_is_rejected() {
    let data = [0u8; 16];

    // The data chunk claims 16 bytes, the ds64 chunk claims 8: contradiction.
    let v = rf64_bytes(8, 16, &data);
    match WavReader::new(Cursor::new(v)) {
        Err(Error::InconsistentDs64) => {}
        other => panic!("expected InconsistentDs64, got {:?}", other.map(|_| ())),
    }

    // 0xffffffff in the data chunk header means: see the ds64 chunk.
    let v = rf64_bytes(16, 0xff_ff_ff_ff, &data);
    let mut reader = WavReader::new(Cursor::new(v)).unwrap();
    assert_eq!(reader.len(), 8);
    assert_eq!(reader.samples::<i16>().count(), 8);

    // A 32-bit data length that agrees with the ds64 chunk is accepted too.
    let v = rf64_bytes(16, 16, &data);
    let mut reader = WavReader::new(Cursor::new(v)).unwrap();
    assert_eq!(reader.len(), 8);
    assert_eq!(reader.samples::<i16>().count(), 8);
}

#[test]
fn rf64_disabled_output_is_byte_identical_riff() {
    // Output of the classic writer (up to 2 GiB and beyond) must be exactly
    // what it was before RF64 support was added.
    let mut buf = Cursor::new(Vec::new());
    {
        let mut writer = WavWriter::new(&mut buf, spec16()).unwrap();
        for s in 0..4_i16 {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
    }

    let expected: Vec<u8> = vec![
        b'R', b'I', b'F', b'F', 44, 0, 0, 0, b'W', b'A', b'V', b'E',
        b'f', b'm', b't', b' ', 16, 0, 0, 0, 1, 0, 1, 0,
        0x44, 0xac, 0x00, 0x00, 0x88, 0x58, 0x01, 0x00, 2, 0, 16, 0,
        b'd', b'a', b't', b'a', 8, 0, 0, 0,
        0, 0, 1, 0, 2, 0, 3, 0,
    ];
    assert_eq!(buf.into_inner(), expected);
}
