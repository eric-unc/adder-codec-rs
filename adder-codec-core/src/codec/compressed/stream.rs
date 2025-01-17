use crate::codec::{CodecError, CodecMetadata, EncoderOptions, ReadCompression, WriteCompression};
use bitstream_io::{BigEndian, BitRead, BitReader, BitWrite, BitWriter};
use std::io::{BufWriter, Cursor, Read, Seek, SeekFrom, Write};

use crate::codec::compressed::source_model::event_structure::event_adu::EventAdu;
use crate::codec::compressed::source_model::HandleEvent;
use crate::codec::header::{Magic, MAGIC_COMPRESSED};
use crate::{DeltaT, Event};

/// Write compressed ADΔER data to a stream.
pub struct CompressedOutput<W: Write> {
    pub(crate) meta: CodecMetadata,
    pub(crate) adu: EventAdu,
    pub(crate) stream: Option<BitWriter<W, BigEndian>>,
    pub(crate) options: EncoderOptions,
}

/// Read compressed ADΔER data from a stream.
pub struct CompressedInput<R: Read> {
    pub(crate) meta: CodecMetadata,

    adu: Option<EventAdu>,

    _phantom: std::marker::PhantomData<R>,
}

impl<W: Write> CompressedOutput<W> {
    /// Create a new compressed output stream.
    pub fn new(meta: CodecMetadata, writer: W) -> Self {
        let adu = EventAdu::new(meta.plane, 0, meta.ref_interval, meta.adu_interval as usize);

        Self {
            meta,
            adu,
            // arithmetic_coder: Some(arithmetic_coder),
            // contexts: Some(contexts),
            stream: Some(BitWriter::endian(writer, BigEndian)),
            options: EncoderOptions::default(meta.plane),
        }
    }

    /// Keep the compressed encoder's option state synchronized with the high-level encoder container
    pub(crate) fn with_options(&mut self, options: EncoderOptions) {
        self.options = options;
    }

    /// Convenience function to get a mutable reference to the underlying stream.
    #[inline(always)]
    pub(crate) fn stream(&mut self) -> &mut BitWriter<W, BigEndian> {
        self.stream.as_mut().unwrap()
    }
}

impl<W: Write> WriteCompression<W> for CompressedOutput<W> {
    fn magic(&self) -> Magic {
        MAGIC_COMPRESSED
    }

    fn meta(&self) -> &CodecMetadata {
        &self.meta
    }

    fn meta_mut(&mut self) -> &mut CodecMetadata {
        &mut self.meta
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), std::io::Error> {
        self.stream().write_bytes(bytes)
    }

    fn byte_align(&mut self) -> std::io::Result<()> {
        self.stream().byte_align()
    }

    fn into_writer(&mut self) -> Option<W> {
        let tmp = self.stream.take();

        tmp.map(|bitwriter| bitwriter.into_writer())
    }

    // fn into_writer(self: Self) -> Option<Box<W>> {
    //     Some(Box::new(self.stream.into_writer()))
    // }

    fn flush_writer(&mut self) -> std::io::Result<()> {
        self.stream().flush()
    }

    fn ingest_event(&mut self, event: Event) -> Result<(), CodecError> {
        // Check that the event fits within the Adu's time range
        if event.t > self.adu.start_t + (self.adu.dt_ref * self.adu.num_intervals as DeltaT) {
            // If it doesn't, compress the events and reset the Adu
            if let Some(stream) = &mut self.stream {
                // Create a temporary u8 stream to write the arithmetic-coded data to
                let mut temp_stream = BitWriter::endian(Vec::new(), BigEndian);

                let parameters = self.options.crf.get_parameters();

                // Compress the Adu. This also writes the EOF symbol and flushes the encoder
                self.adu
                    .compress(&mut temp_stream, parameters.c_thresh_max)?;

                let written_data = temp_stream.into_writer();

                // Write the number of bytes in the compressed Adu as the 32-bit header for this Adu
                stream.write_bytes(&(written_data.len() as u32).to_be_bytes())?;

                // Write the temporary stream to the actual stream
                stream.write_bytes(&written_data)?;
            }
        }

        // Ingest the event in the Adu
        let _ = self.adu.ingest_event(event);

        Ok(())
    }
    // fn ingest_event_debug(&mut self, event: Event) -> Result<Option<Adu>, CodecError> {
    //     if let (true, _) = self.frame.add_event(event, self.meta.delta_t_max)? {
    //         let adu = self.compress_events()?;
    //         self.frame.add_event(event, self.meta.delta_t_max)?;
    //         return Ok(Some(adu));
    //     };
    //     Ok(None)
    // }
}

impl<R: Read> CompressedInput<R> {
    /// Create a new compressed input stream.
    pub fn new(delta_t_max: DeltaT, ref_interval: DeltaT, adu_interval: usize) -> Self
    where
        Self: Sized,
    {
        Self {
            meta: CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: Default::default(),
                plane: Default::default(),
                tps: 0,
                ref_interval,
                delta_t_max,
                event_size: 0,
                source_camera: Default::default(),
                adu_interval,
            },
            adu: None,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<R: Read + Seek> ReadCompression<R> for CompressedInput<R> {
    fn magic(&self) -> Magic {
        MAGIC_COMPRESSED
    }

    fn meta(&self) -> &CodecMetadata {
        &self.meta
    }

    fn meta_mut(&mut self) -> &mut CodecMetadata {
        &mut self.meta
    }

    fn read_bytes(
        &mut self,
        bytes: &mut [u8],
        reader: &mut BitReader<R, BigEndian>,
    ) -> std::io::Result<()> {
        reader.read_bytes(bytes)
    }

    // fn into_reader(self: Box<Self>, reader: &mut BitReader<R, BigEndian>) -> R {
    //     reader.into_reader()
    // }

    #[allow(unused_variables)]
    fn digest_event(&mut self, reader: &mut BitReader<R, BigEndian>) -> Result<Event, CodecError> {
        if self.adu.is_none() {
            self.adu = Some(EventAdu::new(
                self.meta.plane,
                0,
                self.meta.ref_interval,
                self.meta.adu_interval,
            ));
        }

        if let Some(adu) = &mut self.adu {
            if adu.decoder_is_empty() {
                let start = std::time::Instant::now();
                // Read the size of the Adu in bytes
                let mut buffer = [0u8; 4];
                reader.read_bytes(&mut buffer)?;
                let num_bytes = u32::from_be_bytes(buffer);

                // Read the compressed Adu from the stream
                let adu_bytes = reader.read_to_vec(num_bytes as usize)?;

                // Create a temporary u8 stream to read the arithmetic-coded data from
                let mut adu_stream = BitReader::endian(Cursor::new(adu_bytes), BigEndian);

                // Decompress the Adu
                adu.decompress(&mut adu_stream);

                let duration = start.elapsed();
                println!("Decompressed Adu in {:?} ns", duration.as_nanos());
            }
            // Then return the next event from the queue
            match adu.digest_event() {
                Ok(event) => Ok(event),
                Err(CodecError::NoMoreEvents) => {
                    // If there are no more events in the Adu, try decompressing the next Adu
                    self.digest_event(reader)
                }
                Err(e) => Err(e),
            }
        } else {
            unreachable!("Invalid state");
        }
    }

    #[allow(unused_variables)]
    fn set_input_stream_position(
        &mut self,
        reader: &mut BitReader<R, BigEndian>,
        pos: u64,
    ) -> Result<(), CodecError> {
        if pos.saturating_sub(self.meta.header_size as u64) % u64::from(self.meta.event_size) != 0 {
            eprintln!("Attempted to seek to bad position in stream: {pos}");
            return Err(CodecError::Seek);
        }

        if reader.seek_bits(SeekFrom::Start(pos * 8)).is_err() {
            return Err(CodecError::Seek);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::compressed::stream::CompressedInput;
    use crate::codec::{CodecError, ReadCompression};
    use crate::PlaneSize;
    use bitstream_io::{BigEndian, BitReader};
    use std::cmp::min;
    use std::error::Error;
    use std::io;

    /// Test the creation a CompressedOutput and writing a bunch of events to it but NOT getting
    /// to the time where we compress the Adu
    #[test]
    fn test_compress_empty() -> Result<(), Box<dyn Error>> {
        use crate::codec::compressed::stream::CompressedOutput;
        use crate::codec::WriteCompression;
        use crate::Coord;
        use crate::{Event, SourceCamera, TimeMode};
        use std::io::Cursor;

        let start_t = 0;
        let dt_ref = 255;
        let num_intervals = 10;

        let mut compressed_output = CompressedOutput::new(
            crate::codec::CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: TimeMode::AbsoluteT,
                plane: PlaneSize {
                    width: 16,
                    height: 32,
                    channels: 1,
                },
                tps: 7650,
                ref_interval: dt_ref,
                delta_t_max: dt_ref * num_intervals,
                event_size: 0,
                source_camera: SourceCamera::FramedU8,
                adu_interval: num_intervals as usize,
            },
            Cursor::new(Vec::new()),
        );

        let mut counter = 0;
        for y in 0..30 {
            for x in 0..16 {
                compressed_output
                    .ingest_event(Event {
                        coord: Coord { x, y, c: None },
                        t: min(280 + counter, start_t + dt_ref * num_intervals as u32),
                        d: 7,
                    })
                    .unwrap();
                if 280 + counter > start_t + dt_ref * num_intervals as u32 {
                    break;
                } else {
                    counter += 1;
                }
            }
        }

        let output = compressed_output.into_writer().unwrap().into_inner();
        assert!(output.is_empty());
        Ok(())
    }

    #[test]
    fn test_compress_decompress_barely_full() -> Result<(), Box<dyn Error>> {
        use crate::codec::compressed::stream::CompressedOutput;
        use crate::codec::WriteCompression;
        use crate::Coord;
        use crate::{Event, SourceCamera, TimeMode};
        use std::io::Cursor;

        let plane = PlaneSize::new(16, 30, 1)?;
        let start_t = 0;
        let dt_ref = 255;
        let num_intervals = 10;

        // A random candidate pixel to check that its events match
        let candidate_px_idx = (7, 12);
        let mut input_px_events = Vec::new();
        let mut output_px_events = Vec::new();

        let mut compressed_output = CompressedOutput::new(
            crate::codec::CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: TimeMode::AbsoluteT,
                plane: PlaneSize {
                    width: 16,
                    height: 32,
                    channels: 1,
                },
                tps: 7650,
                ref_interval: dt_ref,
                delta_t_max: dt_ref * num_intervals,
                event_size: 0,
                source_camera: SourceCamera::FramedU8,
                adu_interval: num_intervals as usize,
            },
            Cursor::new(Vec::new()),
        );

        let mut counter = 0;
        for y in 0..30 {
            for x in 0..16 {
                let event = Event {
                    coord: Coord { x, y, c: None },
                    t: min(280 + counter, start_t + dt_ref * num_intervals as u32),
                    d: 7,
                };
                if y == candidate_px_idx.0 && x == candidate_px_idx.1 {
                    input_px_events.push(event);
                }
                compressed_output.ingest_event(event).unwrap();
                if 280 + counter > start_t + dt_ref * num_intervals as u32 {
                    break;
                } else {
                    counter += 1;
                }
            }
        }

        // Ingest one more event which is in the next Adu time span
        compressed_output
            .ingest_event(Event {
                coord: Coord {
                    x: 0,
                    y: 0,
                    c: None,
                },
                t: start_t + dt_ref * num_intervals as u32 + 1,
                d: 7,
            })
            .unwrap();
        counter += 1;

        let output = compressed_output.into_writer().unwrap().into_inner();
        assert!(!output.is_empty());
        dbg!(counter);
        // Check that the size is less than the raw events
        assert!((output.len() as u32) < counter * 9);

        let mut compressed_input = CompressedInput::new(
            dt_ref * num_intervals as u32,
            dt_ref,
            num_intervals as usize,
        );
        compressed_input.meta.plane = plane;
        let mut stream = BitReader::endian(Cursor::new(output), BigEndian);
        for _ in 0..counter - 1 {
            let event = compressed_input.digest_event(&mut stream)?;
            if event.coord.y == candidate_px_idx.0 && event.coord.x == candidate_px_idx.1 {
                output_px_events.push(event);
            }
        }

        assert_eq!(input_px_events, output_px_events);
        Ok(())
    }

    #[test]
    fn test_compress_decompress_several() -> Result<(), Box<dyn Error>> {
        use crate::codec::compressed::stream::CompressedOutput;
        use crate::codec::WriteCompression;
        use crate::Coord;
        use crate::{Event, SourceCamera, TimeMode};
        use std::io::Cursor;

        let plane = PlaneSize::new(16, 30, 1)?;
        let dt_ref = 255;
        let num_intervals = 5;

        // A random candidate pixel to check that its events match
        let candidate_px_idx = (7, 12);
        let mut input_px_events = Vec::new();
        let mut output_px_events = Vec::new();

        let mut compressed_output = CompressedOutput::new(
            crate::codec::CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: TimeMode::AbsoluteT,
                plane: PlaneSize {
                    width: 16,
                    height: 32,
                    channels: 1,
                },
                tps: 7650,
                ref_interval: dt_ref,
                delta_t_max: dt_ref * num_intervals as u32,
                event_size: 0,
                source_camera: SourceCamera::FramedU8,
                adu_interval: num_intervals as usize,
            },
            Cursor::new(Vec::new()),
        );

        let mut counter = 0;
        for _ in 0..10 {
            for y in 0..30 {
                for x in 0..16 {
                    let event = Event {
                        coord: Coord { x, y, c: None },
                        t: 280 + counter,
                        d: 7,
                    };
                    if y == candidate_px_idx.0 && x == candidate_px_idx.1 {
                        input_px_events.push(event);
                    }
                    compressed_output.ingest_event(event)?;

                    counter += 1;
                }
            }
        }

        let output = compressed_output.into_writer().unwrap().into_inner();
        assert!(!output.is_empty());
        // Check that the size is less than the raw events
        assert!((output.len() as u32) < counter * 9);

        let mut compressed_input = CompressedInput::new(
            dt_ref * num_intervals as u32,
            dt_ref,
            num_intervals as usize,
        );
        compressed_input.meta.plane = plane;
        let mut stream = BitReader::endian(Cursor::new(output), BigEndian);
        for _ in 0..counter - 1 {
            match compressed_input.digest_event(&mut stream) {
                Ok(event) => {
                    if event.coord.y == candidate_px_idx.0 && event.coord.x == candidate_px_idx.1 {
                        output_px_events.push(event);
                    }
                }
                Err(CodecError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,

                Err(e) => return Err(Box::new(e)),
            }
        }

        assert!(input_px_events.len() >= output_px_events.len());
        for i in 0..output_px_events.len() {
            // Have some slack in the comparison of the T component, since there could be some slight loss here
            let a = input_px_events[i].t - 5..input_px_events[i].t + 5;
            let comp_t = output_px_events[i].t;
            assert!(a.contains(&comp_t));
            assert_eq!(input_px_events[i].d, output_px_events[i].d);
        }
        Ok(())
    }

    #[test]
    fn test_compress_decompress_several_single() -> Result<(), Box<dyn Error>> {
        use crate::codec::compressed::stream::CompressedOutput;
        use crate::codec::WriteCompression;
        use crate::Coord;
        use crate::{Event, SourceCamera, TimeMode};
        use std::io::Cursor;

        let plane = PlaneSize::new(32, 16, 1)?;
        let dt_ref = 255;
        let num_intervals = 5;

        // A random candidate pixel to check that its events match
        let candidate_px_idx = (7, 12);
        let mut input_px_events = Vec::new();
        let mut output_px_events = Vec::new();

        let mut compressed_output = CompressedOutput::new(
            crate::codec::CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: TimeMode::AbsoluteT,
                plane,
                tps: 7650,
                ref_interval: dt_ref,
                delta_t_max: dt_ref * num_intervals as u32,
                event_size: 0,
                source_camera: SourceCamera::FramedU8,
                adu_interval: num_intervals as usize,
            },
            Cursor::new(Vec::new()),
        );

        let mut counter = 0;
        for i in 0..60 {
            let event = Event {
                coord: Coord {
                    x: 12,
                    y: 7,
                    c: None,
                },
                t: 280 + i * 100 + counter,
                d: 7,
            };

            input_px_events.push(event);

            compressed_output.ingest_event(event)?;

            counter += 1;
        }

        // MUCH LATER, integrate an event that with a timestamp far in the past:
        let late_event = Event {
            coord: Coord {
                x: 19,
                y: 14,
                c: None,
            },
            t: 280,
            d: 7,
        };
        compressed_output.ingest_event(late_event)?;

        for i in 60..70 {
            let event = Event {
                coord: Coord {
                    x: 12,
                    y: 7,
                    c: None,
                },
                t: 280 + i * 100 + counter,
                d: 7,
            };

            input_px_events.push(event);

            compressed_output.ingest_event(event)?;

            counter += 1;
        }

        let output = compressed_output.into_writer().unwrap().into_inner();
        assert!(!output.is_empty());
        // Check that the size is less than the raw events

        let mut compressed_input = CompressedInput::new(
            dt_ref * num_intervals as u32,
            dt_ref,
            num_intervals as usize,
        );
        compressed_input.meta.plane = plane;
        let mut stream = BitReader::endian(Cursor::new(output), BigEndian);
        for _ in 0..counter + 1 {
            match compressed_input.digest_event(&mut stream) {
                Ok(event) => {
                    if event.coord.y == candidate_px_idx.0 && event.coord.x == candidate_px_idx.1 {
                        output_px_events.push(event);
                    }
                }
                Err(CodecError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,

                Err(e) => return Err(Box::new(e)),
            }
        }

        assert!(input_px_events.len() >= output_px_events.len());
        for i in 0..output_px_events.len() {
            let span = input_px_events[i].t - 5..input_px_events[i].t + 5;
            let t = output_px_events[i].t;
            assert!(span.contains(&t));
        }
        Ok(())
    }

    #[test]
    fn test_compress_decompress_several_with_skip() -> Result<(), Box<dyn Error>> {
        use crate::codec::compressed::stream::CompressedOutput;
        use crate::codec::WriteCompression;
        use crate::Coord;
        use crate::{Event, SourceCamera, TimeMode};
        use std::io::Cursor;

        let plane = PlaneSize::new(30, 30, 1)?;
        let dt_ref = 255;
        let num_intervals = 10;

        // A random candidate pixel to check that its events match
        let candidate_px_idx = (7, 12);
        let mut input_px_events = Vec::new();
        let mut output_px_events = Vec::new();

        let mut compressed_output = CompressedOutput::new(
            crate::codec::CodecMetadata {
                codec_version: 0,
                header_size: 0,
                time_mode: TimeMode::AbsoluteT,
                plane,
                tps: 7650,
                ref_interval: dt_ref,
                delta_t_max: dt_ref * num_intervals as u32,
                event_size: 0,
                source_camera: SourceCamera::FramedU8,
                adu_interval: num_intervals as usize,
            },
            Cursor::new(Vec::new()),
        );

        let mut counter = 0;
        for i in 0..10 {
            for y in 0..30 {
                for x in 0..30 {
                    // Make the top left cube a skip cube half the time, and skip pixel (14, 14)
                    if !(y == 14 && x == 14 || i % 3 == 0 && y >= 16 && x < 16) {
                        let event = Event {
                            coord: Coord { x, y, c: None },
                            t: 280 + counter,
                            d: 7,
                        };
                        if y == candidate_px_idx.0 && x == candidate_px_idx.1 {
                            input_px_events.push(event);
                        }
                        compressed_output.ingest_event(event)?;

                        counter += 1;
                    }
                }
            }
        }

        // MUCH LATER, integrate an event that with a timestamp far in the past:
        let late_event = Event {
            coord: Coord {
                x: 14,
                y: 14,
                c: None,
            },
            t: 280,
            d: 7,
        };
        compressed_output.ingest_event(late_event)?;

        for i in 0..10 {
            for y in 0..30 {
                for x in 0..30 {
                    // Make the top left cube a skip cube half the time, and skip pixel (14, 14)
                    if !(y == 14 && x == 14 || i % 3 == 0 && y >= 16 && x < 16) {
                        let event = Event {
                            coord: Coord { x, y, c: None },
                            t: 280 + counter,
                            d: 7,
                        };
                        if y == candidate_px_idx.0 && x == candidate_px_idx.1 {
                            input_px_events.push(event);
                        }
                        compressed_output.ingest_event(event)?;

                        counter += 1;
                    }
                }
            }
        }

        let output = compressed_output.into_writer().unwrap().into_inner();
        assert!(!output.is_empty());
        // Check that the size is less than the raw events
        assert!((output.len() as u32) < counter * 9);

        let mut compressed_input = CompressedInput::new(
            dt_ref * num_intervals as u32,
            dt_ref,
            num_intervals as usize,
        );
        compressed_input.meta.plane = plane;
        let mut stream = BitReader::endian(Cursor::new(output), BigEndian);
        loop {
            match compressed_input.digest_event(&mut stream) {
                Ok(event) => {
                    if event.coord.y == candidate_px_idx.0 && event.coord.x == candidate_px_idx.1 {
                        output_px_events.push(event);
                    }
                }
                Err(CodecError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => break,

                Err(e) => return Err(Box::new(e)),
            }
        }

        assert!(input_px_events.len() >= output_px_events.len());
        for i in 0..output_px_events.len() {
            // Have some slack in the comparison of the T component, since there could be some slight loss here
            let a = input_px_events[i].t - 5..input_px_events[i].t + 5;
            let comp_t = output_px_events[i].t;
            assert!(a.contains(&comp_t));
            assert_eq!(input_px_events[i].d, output_px_events[i].d);
        }
        Ok(())
    }
}
