//! An independtly decodable unit of video data.
//!
//! I try to lay out the struct here to be a pretty direct translation of the
//! compressed representation. That is, all the data in the struct is what you get when you
//! decompress an ADU.

use crate::codec::compressed::adu::cube::AduCube;
use crate::codec::compressed::adu::AduCompression;
use crate::codec::compressed::blocks::{DResidual, BLOCK_SIZE_AREA};
use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
use crate::codec::CodecError;
use crate::codec_old::compressed::compression::Contexts;
use crate::codec_old::compressed::fenwick::context_switching::FenwickModel;
use crate::{AbsoluteT, DeltaT, D};
use arithmetic_coding::Encoder;
use bitstream_io::{BigEndian, BitRead, BitReader, BitWriter};
use std::io::{Error, Read, Write};
use std::mem;

pub struct AduChannel {
    /// The number of cubes in the ADU.
    num_cubes: u16,

    /// The cubes in the ADU.
    cubes: Vec<AduCube>,
}

impl AduCompression for AduChannel {
    fn compress<W: Write>(
        &self,
        encoder: &mut Encoder<FenwickModel, BitWriter<W, BigEndian>>,
        contexts: &mut Contexts,
        stream: &mut BitWriter<W, BigEndian>,
        dtm: DeltaT,
    ) -> Result<(), CodecError> {
        // Get the context references
        let mut u8_context = contexts.u8_general_context;

        encoder.model.set_context(u8_context);

        // Write the number of cubes
        for byte in self.num_cubes.to_be_bytes().iter() {
            encoder.encode(Some(&(*byte as usize)), stream)?;
        }

        println!("num_cubes: {}", self.num_cubes);

        // Write the cubes
        for cube in self.cubes.iter() {
            cube.compress(encoder, contexts, stream, dtm)?;
        }

        Ok(())
    }

    fn decompress<R: Read>(
        stream: &mut BitReader<R, BigEndian>,
        input: &mut CompressedInput<R>,
    ) -> Self {
        // Get the context references
        let mut decoder = input.arithmetic_coder.as_mut().unwrap();
        let mut d_context = input.contexts.as_mut().unwrap().d_context;
        let mut dt_context = input.contexts.as_mut().unwrap().dt_context;
        let mut u8_context = input.contexts.as_mut().unwrap().u8_general_context;

        decoder.model.set_context(u8_context);

        // Read the number of cubes
        let mut bytes = [0; 2];
        for byte in bytes.iter_mut() {
            *byte = decoder.decode(stream).unwrap().unwrap() as u8;
        }
        let num_cubes = u16::from_be_bytes(bytes);

        // Read the cubes
        let mut cubes = Vec::new();
        for _ in 0..num_cubes {
            cubes.push(AduCube::decompress(stream, input));
        }

        Self { num_cubes, cubes }
    }
}

/// A whole spatial frame of data
pub struct Adu {
    /// The timestamp of the first event in the ADU.
    pub(crate) head_event_t: AbsoluteT,

    pub(crate) cubes_r: AduChannel,
    pub(crate) cubes_g: AduChannel,
    pub(crate) cubes_b: AduChannel,
}

pub enum AduChannelType {
    R,
    G,
    B,
}

impl Adu {
    pub fn new() -> Self {
        Self {
            head_event_t: 0,
            cubes_r: AduChannel {
                num_cubes: 0,
                cubes: Vec::new(),
            },
            cubes_g: AduChannel {
                num_cubes: 0,
                cubes: Vec::new(),
            },
            cubes_b: AduChannel {
                num_cubes: 0,
                cubes: Vec::new(),
            },
        }
    }

    pub fn add_cube(&mut self, cube: AduCube, channel: AduChannelType) {
        match channel {
            AduChannelType::R => {
                self.cubes_r.cubes.push(cube);
                self.cubes_r.num_cubes += 1;
            }
            AduChannelType::G => {
                self.cubes_g.cubes.push(cube);
                self.cubes_g.num_cubes += 1;
            }
            AduChannelType::B => {
                self.cubes_b.cubes.push(cube);
                self.cubes_b.num_cubes += 1;
            }
        }
    }
}

impl AduCompression for Adu {
    fn compress<W: Write>(
        &self,
        encoder: &mut Encoder<FenwickModel, BitWriter<W, BigEndian>>,
        contexts: &mut Contexts,
        stream: &mut BitWriter<W, BigEndian>,
        dtm: DeltaT,
    ) -> Result<(), CodecError> {
        // Get the context references
        let mut u8_context = contexts.u8_general_context;

        encoder.model.set_context(u8_context);

        // Write the head event timestamp
        for byte in self.head_event_t.to_be_bytes().iter() {
            encoder.encode(Some(&(*byte as usize)), stream)?;
        }

        // Write the cubes
        self.cubes_r.compress(encoder, contexts, stream, dtm)?;
        self.cubes_g.compress(encoder, contexts, stream, dtm)?;
        self.cubes_b.compress(encoder, contexts, stream, dtm)?;

        Ok(())
    }

    fn decompress<R: Read>(
        stream: &mut BitReader<R, BigEndian>,
        input: &mut CompressedInput<R>,
    ) -> Self {
        // Get the context references
        let mut decoder = input.arithmetic_coder.as_mut().unwrap();
        let mut d_context = input.contexts.as_mut().unwrap().d_context;
        let mut dt_context = input.contexts.as_mut().unwrap().dt_context;
        let mut u8_context = input.contexts.as_mut().unwrap().u8_general_context;

        decoder.model.set_context(u8_context);

        // Read the head event timestamp
        let mut bytes = [0; mem::size_of::<AbsoluteT>()];
        for byte in bytes.iter_mut() {
            *byte = decoder.decode(stream).unwrap().unwrap() as u8;
        }
        let head_event_t = AbsoluteT::from_be_bytes(bytes);

        // Read the cubes
        let cubes_r = AduChannel::decompress(stream, input);
        let cubes_g = AduChannel::decompress(stream, input);
        let cubes_b = AduChannel::decompress(stream, input);

        Self {
            head_event_t,
            cubes_r,
            cubes_g,
            cubes_b,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::codec::compressed::adu::cube::AduCube;
    use crate::codec::compressed::adu::frame::{compare_channels, Adu, AduChannel};
    use crate::codec::compressed::adu::interblock::AduInterBlock;
    use crate::codec::compressed::adu::intrablock::gen_random_intra_block;
    use crate::codec::compressed::adu::AduCompression;
    use crate::codec::compressed::stream::{CompressedInput, CompressedOutput};
    use crate::codec::{CodecMetadata, WriteCompression};
    use rand::prelude::StdRng;
    use rand::{Rng, SeedableRng};
    use std::error::Error;
    use std::io::BufReader;

    fn setup_encoder() -> crate::codec::compressed::stream::CompressedOutput<Vec<u8>> {
        let meta = CodecMetadata {
            delta_t_max: 100,
            ref_interval: 100,
            ..Default::default()
        };
        // By building the CompressedOutput directly (rather than calling Encoder::new_compressed),
        // we can avoid writing the header and stuff for testing purposes.
        crate::codec::compressed::stream::CompressedOutput::new(meta, Vec::new())
    }

    fn gen_rand_channel(
        encoder: &mut CompressedOutput<Vec<u8>>,
        seed: Option<u64>,
        mut rng: StdRng,
    ) -> AduChannel {
        let mut cubes = Vec::new();
        for _ in 0..10 {
            let intra_block = gen_random_intra_block(1234, encoder.meta.delta_t_max, seed);
            let mut cube = crate::codec::compressed::adu::cube::AduCube::from_intra_block(
                intra_block,
                rng.gen(),
                rng.gen(),
            );

            for _ in 0..10 {
                let intra_block = gen_random_intra_block(1234, encoder.meta.delta_t_max, seed);
                // For convenience, we'll just use the intra block's generator.
                let inter_block = AduInterBlock {
                    shift_loss_param: intra_block.shift_loss_param,
                    d_residuals: intra_block.d_residuals,
                    t_residuals: intra_block.dt_residuals,
                };
                cube.add_inter_block(inter_block);
            }
            cubes.push(cube);
        }

        let mut channel = AduChannel {
            num_cubes: cubes.len() as u16,
            cubes,
        };
        channel
    }

    fn setup_channel(encoder: &mut CompressedOutput<Vec<u8>>, seed: Option<u64>) -> AduChannel {
        let mut rng = match seed {
            None => StdRng::from_rng(rand::thread_rng()).unwrap(),
            Some(num) => StdRng::seed_from_u64(num),
        };

        gen_rand_channel(encoder, seed, rng)
    }

    fn compress_channel() -> Result<(AduChannel, Vec<u8>), Box<dyn Error>> {
        let mut encoder = setup_encoder();
        let channel = setup_channel(&mut encoder, Some(7));

        assert!(channel
            .compress(
                encoder.arithmetic_coder.as_mut().unwrap(),
                encoder.contexts.as_mut().unwrap(),
                encoder.stream.as_mut().unwrap(),
                encoder.meta.delta_t_max
            )
            .is_ok());

        let written_data = encoder.into_writer().unwrap();

        Ok((channel, written_data))
    }

    fn setup_adu(encoder: &mut CompressedOutput<Vec<u8>>, seed: Option<u64>) -> Adu {
        let mut rng = match seed {
            None => StdRng::from_rng(rand::thread_rng()).unwrap(),
            Some(num) => StdRng::seed_from_u64(num),
        };

        let cubes_r = gen_rand_channel(encoder, seed, rng.clone());
        let cubes_g = gen_rand_channel(encoder, seed, rng.clone());
        let cubes_b = gen_rand_channel(encoder, seed, rng.clone());

        Adu {
            head_event_t: rng.gen(),
            cubes_r,
            cubes_g,
            cubes_b,
        }
    }

    fn compress_adu() -> Result<(Adu, Vec<u8>), Box<dyn Error>> {
        let mut encoder = setup_encoder();

        let adu = setup_adu(&mut encoder, Some(7));

        assert!(adu
            .compress(
                encoder.arithmetic_coder.as_mut().unwrap(),
                encoder.contexts.as_mut().unwrap(),
                encoder.stream.as_mut().unwrap(),
                encoder.meta.delta_t_max
            )
            .is_ok());

        let written_data = encoder.into_writer().unwrap();

        Ok((adu, written_data))
    }

    #[test]
    fn test_compress_channel() {
        let (_, written_data) = compress_channel().unwrap();
        let output_len = written_data.len();
        let input_len = 1028 * 11 * 10; // Rough approximation
        assert!(output_len < input_len);
        eprintln!("Output length: {}", output_len);
        eprintln!("Input length: {}", input_len);
    }

    #[test]
    fn test_decompress_channel() {
        let (channel, written_data) = compress_channel().unwrap();
        let tmp_len = written_data.len();

        let mut bufreader = BufReader::new(written_data.as_slice());
        let mut bitreader =
            bitstream_io::BitReader::endian(&mut bufreader, bitstream_io::BigEndian);

        let mut decoder = CompressedInput::new(100, 100);

        let decoded_channel = AduChannel::decompress(&mut bitreader, &mut decoder);

        decoder
            .arithmetic_coder
            .as_mut()
            .unwrap()
            .model
            .set_context(decoder.contexts.as_mut().unwrap().eof_context);
        let eof = decoder
            .arithmetic_coder
            .as_mut()
            .unwrap()
            .decode(&mut bitreader)
            .unwrap();
        assert!(eof.is_none());
        compare_channels(&channel, &decoded_channel);
    }

    #[test]
    fn test_compress_adu() {
        let (_, written_data) = compress_adu().unwrap();
        let output_len = written_data.len();
        let input_len = 1028 * 11 * 10 * 3; // Rough approximation
        assert!(output_len < input_len);
        eprintln!("Output length: {}", output_len);
        eprintln!("Input length: {}", input_len);
    }

    #[test]
    fn test_decompress_adu() {
        let (adu, written_data) = compress_adu().unwrap();
        let tmp_len = written_data.len();

        let mut bufreader = BufReader::new(written_data.as_slice());
        let mut bitreader =
            bitstream_io::BitReader::endian(&mut bufreader, bitstream_io::BigEndian);

        let mut decoder = CompressedInput::new(100, 100);

        let decoded_adu = Adu::decompress(&mut bitreader, &mut decoder);

        decoder
            .arithmetic_coder
            .as_mut()
            .unwrap()
            .model
            .set_context(decoder.contexts.as_mut().unwrap().eof_context);
        let eof = decoder
            .arithmetic_coder
            .as_mut()
            .unwrap()
            .decode(&mut bitreader)
            .unwrap();
        assert!(eof.is_none());
        assert_eq!(adu.head_event_t, decoded_adu.head_event_t);

        compare_channels(&adu.cubes_r, &decoded_adu.cubes_r);
        compare_channels(&adu.cubes_g, &decoded_adu.cubes_g);
        compare_channels(&adu.cubes_b, &decoded_adu.cubes_b);
    }
}

/// Helper function for test code
pub fn compare_channels(channel: &AduChannel, decoded_channel: &AduChannel) {
    assert_eq!(channel.num_cubes, decoded_channel.num_cubes);

    for (cube, decoded_cube) in channel.cubes.iter().zip(decoded_channel.cubes.iter()) {
        assert_eq!(cube.idx_y, decoded_cube.idx_y);
        assert_eq!(cube.idx_x, decoded_cube.idx_x);
        assert_eq!(
            cube.intra_block.head_event_t,
            decoded_cube.intra_block.head_event_t
        );
        assert_eq!(
            cube.intra_block.head_event_d,
            decoded_cube.intra_block.head_event_d
        );
        assert_eq!(
            cube.intra_block.shift_loss_param,
            decoded_cube.intra_block.shift_loss_param
        );
        assert_eq!(
            cube.intra_block.d_residuals,
            decoded_cube.intra_block.d_residuals
        );
        assert_eq!(
            cube.intra_block.dt_residuals,
            decoded_cube.intra_block.dt_residuals
        );
        assert_eq!(cube.num_inter_blocks, decoded_cube.num_inter_blocks);
        for (block, decoded_block) in cube.inter_blocks.iter().zip(&decoded_cube.inter_blocks) {
            assert_eq!(block.shift_loss_param, decoded_block.shift_loss_param);
            assert_eq!(block.d_residuals, decoded_block.d_residuals);
            assert_eq!(block.t_residuals, decoded_block.t_residuals);
        }
    }
}