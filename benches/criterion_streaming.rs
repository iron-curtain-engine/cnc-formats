use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};

use cnc_formats::{aud, big, mix, vqa};
use std::hint::black_box;

#[cfg(feature = "convert")]
use cnc_formats::convert;
#[cfg(feature = "meg")]
use cnc_formats::meg;

use std::io::{self, Cursor};

#[path = "support/archives.rs"]
mod archives;
#[cfg(feature = "convert")]
#[path = "support/export_fixtures.rs"]
mod export_fixtures;
#[path = "support/stream_media.rs"]
mod stream_media;

fn bench_archive_streaming(c: &mut Criterion) {
    let mix_fixture = archives::mix_fixture();
    let big_fixture = archives::big_fixture();

    let mut group = c.benchmark_group("archive_streaming");

    group.throughput(Throughput::Bytes(mix_fixture.bytes.len() as u64));
    group.bench_function("mix_reader_open", |b| {
        b.iter(|| {
            let reader =
                mix::MixArchiveReader::open(Cursor::new(mix_fixture.bytes.as_slice())).unwrap();
            black_box(reader.file_count())
        })
    });
    group.bench_function("mix_reader_copy", |b| {
        b.iter_batched(
            || mix::MixArchiveReader::open(Cursor::new(mix_fixture.bytes.as_slice())).unwrap(),
            |mut reader| {
                let mut sink = io::sink();
                black_box(
                    reader
                        .copy_by_crc(mix_fixture.query_crc, &mut sink)
                        .unwrap(),
                )
            },
            BatchSize::SmallInput,
        )
    });

    group.throughput(Throughput::Bytes(big_fixture.bytes.len() as u64));
    group.bench_function("big_reader_open", |b| {
        b.iter(|| {
            let reader =
                big::BigArchiveReader::open(Cursor::new(big_fixture.bytes.as_slice())).unwrap();
            black_box(reader.file_count())
        })
    });
    group.bench_function("big_reader_copy", |b| {
        b.iter_batched(
            || big::BigArchiveReader::open(Cursor::new(big_fixture.bytes.as_slice())).unwrap(),
            |mut reader| {
                let mut sink = io::sink();
                black_box(
                    reader
                        .copy_by_index(big_fixture.query_index, &mut sink)
                        .unwrap(),
                )
            },
            BatchSize::SmallInput,
        )
    });

    #[cfg(feature = "meg")]
    {
        let meg_fixture = archives::meg_fixture();
        group.throughput(Throughput::Bytes(meg_fixture.bytes.len() as u64));
        group.bench_function("meg_reader_open", |b| {
            b.iter(|| {
                let reader =
                    meg::MegArchiveReader::open(Cursor::new(meg_fixture.bytes.as_slice())).unwrap();
                black_box(reader.file_count())
            })
        });
        group.bench_function("meg_reader_copy", |b| {
            b.iter_batched(
                || meg::MegArchiveReader::open(Cursor::new(meg_fixture.bytes.as_slice())).unwrap(),
                |mut reader| {
                    let mut sink = io::sink();
                    black_box(
                        reader
                            .copy_by_index(meg_fixture.query_index, &mut sink)
                            .unwrap(),
                    )
                },
                BatchSize::SmallInput,
            )
        });
    }

    group.finish();
}

fn bench_aud_streaming(c: &mut Criterion) {
    let aud_bytes = stream_media::aud_bytes();
    let mut group = c.benchmark_group("aud_streaming");
    group.throughput(Throughput::Bytes(aud_bytes.len() as u64));

    group.bench_function("aud_read_samples_drain", |b| {
        b.iter_batched(
            || aud::AudStream::open_seekable(Cursor::new(aud_bytes)).unwrap(),
            |mut stream| {
                let mut scratch = [0i16; 2048];
                let mut total = 0usize;
                loop {
                    let read = stream.read_samples(&mut scratch).unwrap();
                    if read == 0 {
                        break;
                    }
                    total = total.saturating_add(read);
                }
                black_box(total)
            },
            BatchSize::SmallInput,
        )
    });

    group.bench_function("aud_next_chunk_drain", |b| {
        b.iter_batched(
            || aud::AudStream::open_seekable(Cursor::new(aud_bytes)).unwrap(),
            |mut stream| {
                let mut total = 0usize;
                while let Some(chunk) = stream.next_chunk(1024).unwrap() {
                    total = total.saturating_add(chunk.samples.len());
                }
                black_box(total)
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

fn bench_vqa_streaming(c: &mut Criterion) {
    let fixture = stream_media::vqa_fixture();
    let mut group = c.benchmark_group("vqa_streaming");
    group.throughput(Throughput::Bytes(fixture.bytes.len() as u64));

    group.bench_function("vqa_stream_next_chunk_drain", |b| {
        b.iter_batched(
            || vqa::VqaStream::open(Cursor::new(fixture.bytes.as_slice())).unwrap(),
            |mut stream| {
                let mut total = 0usize;
                while let Some(chunk) = stream.next_chunk().unwrap() {
                    total = total.saturating_add(chunk.data.len());
                }
                black_box(total)
            },
            BatchSize::SmallInput,
        )
    });

    group.bench_function("vqa_decoder_next_frame_into_drain", |b| {
        b.iter_batched(
            || {
                let decoder = vqa::VqaDecoder::open(Cursor::new(fixture.bytes.as_slice())).unwrap();
                let buffer = vqa::VqaFrameBuffer::from_media_info(&decoder.media_info());
                (decoder, buffer)
            },
            |(mut decoder, mut buffer)| {
                let mut frames = 0usize;
                while decoder.next_frame_into(&mut buffer).unwrap().is_some() {
                    frames = frames.saturating_add(1);
                }
                black_box(frames)
            },
            BatchSize::SmallInput,
        )
    });

    group.bench_function("vqa_decoder_read_audio_samples_drain", |b| {
        b.iter_batched(
            || vqa::VqaDecoder::open(Cursor::new(fixture.bytes.as_slice())).unwrap(),
            |mut decoder| {
                let mut scratch = [0i16; 2048];
                let mut total = 0usize;
                loop {
                    let read = decoder.read_audio_samples(&mut scratch).unwrap();
                    if read == 0 {
                        break;
                    }
                    total = total.saturating_add(read);
                }
                black_box(total)
            },
            BatchSize::SmallInput,
        )
    });

    group.bench_function("vqa_decoder_audio_for_frame_interval_drain", |b| {
        b.iter_batched(
            || vqa::VqaDecoder::open(Cursor::new(fixture.bytes.as_slice())).unwrap(),
            |mut decoder| {
                let mut total = 0usize;
                while let Some(chunk) = decoder.next_audio_for_frame_interval().unwrap() {
                    total = total.saturating_add(chunk.samples.len());
                }
                black_box(total)
            },
            BatchSize::SmallInput,
        )
    });

    group.bench_function("vqa_decoder_seek_to_frame_middle", |b| {
        b.iter_batched(
            || vqa::VqaDecoder::open(Cursor::new(fixture.bytes.as_slice())).unwrap(),
            |mut decoder| {
                decoder
                    .seek_to_frame(stream_media::vqa_seek_target_frame())
                    .unwrap();
                black_box(decoder.decoded_audio_sample_frames())
            },
            BatchSize::SmallInput,
        )
    });

    group.finish();
}

#[cfg(feature = "convert")]
fn bench_export(c: &mut Criterion) {
    use cnc_formats::{aud::AudFile, pal::Palette, shp::ShpFile, wsa::WsaFile};

    let aud_bytes = export_fixtures::aud_bytes();
    let shp_bytes = export_fixtures::shp_bytes();
    let wsa_bytes = export_fixtures::wsa_bytes();
    let vqa_fixture = export_fixtures::vqa_fixture();
    let pal_bytes = export_fixtures::pal_bytes();

    let aud_file = AudFile::parse(aud_bytes).unwrap();
    let shp_file = ShpFile::parse(shp_bytes).unwrap();
    let wsa_file = WsaFile::parse(wsa_bytes).unwrap();
    let palette = Palette::parse(pal_bytes).unwrap();
    let vqa_file = vqa::VqaFile::parse(&vqa_fixture.bytes).unwrap();

    let mut group = c.benchmark_group("export");

    group.throughput(Throughput::Bytes(aud_bytes.len() as u64));
    group.bench_function("aud_to_wav", |b| {
        b.iter(|| black_box(convert::aud_to_wav(&aud_file).unwrap().len()))
    });

    group.throughput(Throughput::Bytes(shp_bytes.len() as u64));
    group.bench_function("shp_to_png", |b| {
        b.iter(|| {
            black_box(
                convert::shp_frames_to_png(&shp_file, &palette)
                    .unwrap()
                    .len(),
            )
        })
    });

    group.bench_function("shp_to_gif", |b| {
        b.iter(|| {
            black_box(
                convert::shp_frames_to_gif(&shp_file, &palette, 10)
                    .unwrap()
                    .len(),
            )
        })
    });

    group.throughput(Throughput::Bytes(wsa_bytes.len() as u64));
    group.bench_function("wsa_to_png", |b| {
        b.iter(|| {
            black_box(
                convert::wsa_frames_to_png(&wsa_file, &palette)
                    .unwrap()
                    .len(),
            )
        })
    });

    group.bench_function("wsa_to_gif", |b| {
        b.iter(|| {
            black_box(
                convert::wsa_frames_to_gif(&wsa_file, &palette, 10)
                    .unwrap()
                    .len(),
            )
        })
    });

    group.throughput(Throughput::Bytes(vqa_fixture.bytes.len() as u64));
    group.bench_function("vqa_to_avi", |b| {
        b.iter(|| black_box(convert::vqa_to_avi(&vqa_file).unwrap().len()))
    });

    group.bench_function("vqa_to_mkv", |b| {
        b.iter(|| {
            black_box(
                convert::vqa_to_mkv(&vqa_file, convert::MkvVideoCodec::default())
                    .unwrap()
                    .len(),
            )
        })
    });

    group.throughput(Throughput::Bytes(pal_bytes.len() as u64));
    group.bench_function("pal_to_png", |b| {
        b.iter(|| black_box(convert::pal_to_png(&palette).unwrap().len()))
    });

    group.finish();
}

#[cfg(feature = "convert")]
criterion_group!(
    streaming_benches,
    bench_archive_streaming,
    bench_aud_streaming,
    bench_vqa_streaming,
    bench_export
);

#[cfg(not(feature = "convert"))]
criterion_group!(
    streaming_benches,
    bench_archive_streaming,
    bench_aud_streaming,
    bench_vqa_streaming
);

criterion_main!(streaming_benches);
