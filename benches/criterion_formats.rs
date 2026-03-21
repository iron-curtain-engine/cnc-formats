use criterion::measurement::WallTime;
use criterion::{criterion_group, criterion_main, BenchmarkGroup, Criterion, Throughput};

use cnc_formats::{
    aud, big, cps, csf, dip, eng, fnt, hva, ini, lut, mix, pal, shp, shp_ts, tmp, vqa, vqp, vxl,
    w3d, wsa,
};
use std::hint::black_box;

#[cfg(feature = "adl")]
use cnc_formats::adl;
#[cfg(feature = "meg")]
use cnc_formats::meg;
#[cfg(feature = "midi")]
use cnc_formats::mid;
#[cfg(feature = "miniyaml")]
use cnc_formats::miniyaml;
#[cfg(feature = "xmi")]
use cnc_formats::xmi;

#[path = "support/archives.rs"]
mod archives;
#[path = "support/extra_formats.rs"]
mod extra_formats;
#[path = "support/media.rs"]
mod media;
#[path = "support/music.rs"]
mod music;
#[path = "support/text.rs"]
mod text;

fn bench_parse_case<F>(
    group: &mut BenchmarkGroup<'_, WallTime>,
    name: &str,
    bytes: &[u8],
    mut parse: F,
) where
    F: FnMut(&[u8]) -> bool,
{
    group.throughput(Throughput::Bytes(bytes.len() as u64));
    group.bench_function(name, |b| b.iter(|| black_box(parse(black_box(bytes)))));
}

fn bench_hot_lookup(c: &mut Criterion) {
    let fixture = archives::mix_fixture();
    let archive = mix::MixArchive::parse(&fixture.bytes).unwrap();

    let mut group = c.benchmark_group("hot_lookup");
    group.bench_function("mix_crc", |b| {
        b.iter(|| black_box(mix::crc(black_box("TARGET.BIN"))))
    });
    group.bench_function("mix_get_by_crc", |b| {
        b.iter(|| black_box(archive.get_by_crc(black_box(fixture.query_crc))))
    });
    group.finish();
}

fn bench_archive_parse(c: &mut Criterion) {
    let mix_fixture = archives::mix_fixture();
    let big_fixture = archives::big_fixture();
    black_box(big_fixture.query_index);

    let mut group = c.benchmark_group("archive_parse");
    bench_parse_case(&mut group, "mix_parse", &mix_fixture.bytes, |data| {
        mix::MixArchive::parse(data).is_ok()
    });
    bench_parse_case(&mut group, "big_parse", &big_fixture.bytes, |data| {
        big::BigArchive::parse(data).is_ok()
    });
    #[cfg(feature = "meg")]
    {
        let meg_fixture = archives::meg_fixture();
        black_box(meg_fixture.query_index);
        bench_parse_case(&mut group, "meg_parse", &meg_fixture.bytes, |data| {
            meg::MegArchive::parse(data).is_ok()
        });
    }
    group.finish();
}

fn bench_media_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("media_parse");
    bench_parse_case(&mut group, "pal_parse", text::pal_bytes(), |data| {
        pal::Palette::parse(data).is_ok()
    });
    bench_parse_case(&mut group, "aud_parse", media::aud_bytes(), |data| {
        aud::AudFile::parse(data).is_ok()
    });
    bench_parse_case(&mut group, "shp_parse", media::shp_bytes(), |data| {
        shp::ShpFile::parse(data).is_ok()
    });
    bench_parse_case(&mut group, "wsa_parse", media::wsa_bytes(), |data| {
        wsa::WsaFile::parse(data).is_ok()
    });
    bench_parse_case(&mut group, "td_tmp_parse", media::td_tmp_bytes(), |data| {
        tmp::TdTmpFile::parse(data).is_ok()
    });
    bench_parse_case(&mut group, "ra_tmp_parse", media::ra_tmp_bytes(), |data| {
        tmp::RaTmpFile::parse(data).is_ok()
    });
    bench_parse_case(
        &mut group,
        "vqa_parse",
        &media::vqa_fixture().bytes,
        |data| vqa::VqaFile::parse(data).is_ok(),
    );
    bench_parse_case(&mut group, "vqp_parse", text::vqp_bytes(), |data| {
        vqp::VqpFile::parse(data).is_ok()
    });
    bench_parse_case(&mut group, "fnt_parse", text::fnt_bytes(), |data| {
        fnt::FntFile::parse(data).is_ok()
    });
    bench_parse_case(&mut group, "eng_parse", text::eng_bytes(), |data| {
        eng::EngFile::parse(data).is_ok()
    });
    bench_parse_case(
        &mut group,
        "dip_parse",
        text::dip_segmented_bytes(),
        |data| dip::DipFile::parse(data).is_ok(),
    );
    bench_parse_case(&mut group, "lut_parse", text::lut_bytes(), |data| {
        lut::LutFile::parse(data).is_ok()
    });
    bench_parse_case(&mut group, "ini_parse", text::ini_bytes(), |data| {
        ini::IniFile::parse(data).is_ok()
    });
    #[cfg(feature = "miniyaml")]
    bench_parse_case(
        &mut group,
        "miniyaml_parse",
        text::miniyaml_bytes(),
        |data| miniyaml::MiniYamlDoc::parse(data).is_ok(),
    );
    #[cfg(feature = "midi")]
    bench_parse_case(&mut group, "mid_parse", music::mid_bytes(), |data| {
        mid::MidFile::parse(data).is_ok()
    });
    #[cfg(feature = "adl")]
    bench_parse_case(&mut group, "adl_parse", music::adl_bytes(), |data| {
        adl::AdlFile::parse(data).is_ok()
    });
    #[cfg(feature = "xmi")]
    bench_parse_case(&mut group, "xmi_parse", music::xmi_bytes(), |data| {
        xmi::XmiFile::parse(data).is_ok()
    });
    bench_parse_case(
        &mut group,
        "csf_parse",
        extra_formats::csf_bytes(),
        |data| csf::CsfFile::parse(data).is_ok(),
    );
    bench_parse_case(
        &mut group,
        "cps_parse",
        extra_formats::cps_bytes(),
        |data| cps::CpsFile::parse(data).is_ok(),
    );
    bench_parse_case(
        &mut group,
        "shp_ts_parse",
        extra_formats::shp_ts_bytes(),
        |data| shp_ts::ShpTsFile::parse(data).is_ok(),
    );
    bench_parse_case(
        &mut group,
        "vxl_parse",
        extra_formats::vxl_bytes(),
        |data| vxl::VxlFile::parse(data).is_ok(),
    );
    bench_parse_case(
        &mut group,
        "hva_parse",
        extra_formats::hva_bytes(),
        |data| hva::HvaFile::parse(data).is_ok(),
    );
    bench_parse_case(
        &mut group,
        "w3d_parse",
        extra_formats::w3d_bytes(),
        |data| w3d::W3dFile::parse(data).is_ok(),
    );
    group.finish();
}

criterion_group!(
    format_benches,
    bench_hot_lookup,
    bench_archive_parse,
    bench_media_parse
);
criterion_main!(format_benches);
