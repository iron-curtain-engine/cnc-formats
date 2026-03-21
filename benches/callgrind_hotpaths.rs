#[cfg(not(unix))]
fn main() {}

#[cfg(unix)]
#[path = "support/mix_only.rs"]
mod mix_only;
#[cfg(unix)]
#[path = "support/stream_media.rs"]
mod stream_media;

#[cfg(unix)]
mod hotpaths {
    use crate::{mix_only, stream_media};
    use cnc_formats::{aud, mix, vqa};
    use iai_callgrind::{library_benchmark, library_benchmark_group};
    use std::hint::black_box;
    use std::io::Cursor;

    struct MixLookupFixture {
        archive: mix::MixArchive<'static>,
        key: mix::MixCrc,
    }

    struct AudReadFixture {
        stream: aud::AudStream<Cursor<&'static [u8]>>,
        scratch: [i16; 2048],
    }

    struct VqaAudioReadFixture {
        decoder: vqa::VqaDecoder<Cursor<&'static [u8]>>,
        scratch: [i16; 2048],
    }

    struct VqaFrameFixture {
        decoder: vqa::VqaDecoder<Cursor<&'static [u8]>>,
        buffer: vqa::VqaFrameBuffer,
    }

    fn setup_mix_lookup() -> MixLookupFixture {
        let fixture = mix_only::mix_fixture();
        let bytes = fixture.bytes.as_slice();
        MixLookupFixture {
            archive: mix::MixArchive::parse(bytes).unwrap(),
            key: fixture.query_crc,
        }
    }

    fn setup_aud_read() -> AudReadFixture {
        AudReadFixture {
            stream: aud::AudStream::open_seekable(Cursor::new(stream_media::aud_bytes())).unwrap(),
            scratch: [0; 2048],
        }
    }

    fn setup_vqa_audio_read() -> VqaAudioReadFixture {
        let fixture = stream_media::vqa_fixture();
        let _ = stream_media::vqa_seek_target_frame();
        let mut decoder = vqa::VqaDecoder::open(Cursor::new(fixture.bytes.as_slice())).unwrap();
        let mut frame = vqa::VqaFrameBuffer::from_media_info(&decoder.media_info());
        let _ = decoder.next_frame_into(&mut frame).unwrap();
        VqaAudioReadFixture {
            decoder,
            scratch: [0; 2048],
        }
    }

    fn setup_vqa_frame_step() -> VqaFrameFixture {
        let fixture = stream_media::vqa_fixture();
        let _ = stream_media::vqa_seek_target_frame();
        let decoder = vqa::VqaDecoder::open(Cursor::new(fixture.bytes.as_slice())).unwrap();
        let buffer = vqa::VqaFrameBuffer::from_media_info(&decoder.media_info());
        VqaFrameFixture { decoder, buffer }
    }

    #[library_benchmark]
    fn mix_crc_hot() -> u32 {
        black_box(mix::crc("TARGET.BIN").to_raw())
    }

    #[library_benchmark]
    #[bench::lookup(setup_mix_lookup())]
    fn mix_get_by_crc_hot(fixture: MixLookupFixture) -> usize {
        black_box(
            fixture
                .archive
                .get_by_crc(fixture.key)
                .map_or(0, <[u8]>::len),
        )
    }

    #[library_benchmark]
    #[bench::read(setup_aud_read())]
    fn aud_read_samples_hot(mut fixture: AudReadFixture) -> usize {
        black_box(fixture.stream.read_samples(&mut fixture.scratch).unwrap())
    }

    #[library_benchmark]
    #[bench::read(setup_vqa_audio_read())]
    fn vqa_read_audio_samples_hot(mut fixture: VqaAudioReadFixture) -> usize {
        black_box(
            fixture
                .decoder
                .read_audio_samples(&mut fixture.scratch)
                .unwrap(),
        )
    }

    #[library_benchmark]
    #[bench::step(setup_vqa_frame_step())]
    fn vqa_next_frame_into_hot(mut fixture: VqaFrameFixture) -> Option<u16> {
        black_box(
            fixture
                .decoder
                .next_frame_into(&mut fixture.buffer)
                .unwrap(),
        )
    }

    library_benchmark_group!(
        name = hotpath_group;
        benchmarks =
            mix_crc_hot,
            mix_get_by_crc_hot,
            aud_read_samples_hot,
            vqa_read_audio_samples_hot,
            vqa_next_frame_into_hot
    );
}

#[cfg(unix)]
use hotpaths::*;

#[cfg(unix)]
iai_callgrind::main!(library_benchmark_groups = hotpath_group);
