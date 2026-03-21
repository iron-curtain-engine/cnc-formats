// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

//! Export conversions: C&C formats → common formats (PNG, WAV, GIF).

#[cfg(feature = "convert")]
use super::super::{
    open_file, read_file, report_convert_error, report_parse_error, resolve_format, Format,
};
#[cfg(feature = "convert")]
use super::{
    derive_output, derive_output_dir, require_palette, write_png_sequence, write_single_file,
};

// ── Export conversions (C&C → common) ────────────────────────────────────────

/// Convert a file to PNG based on its detected source format.
#[cfg(feature = "convert")]
pub(super) fn convert_to_png(
    path: &str,
    explicit: Option<Format>,
    palette_path: Option<&str>,
    output: Option<&str>,
) -> i32 {
    let fmt = resolve_format(path, explicit);
    let data = read_file(path);

    match fmt {
        Format::Shp => {
            let pal = require_palette(palette_path, "SHP");
            match cnc_formats::shp::ShpFile::parse(&data) {
                Ok(shp) => {
                    eprintln!(
                        "Converting SHP → PNG ({} frames, {}×{})...",
                        shp.header.frame_count, shp.header.width, shp.header.height
                    );
                    match cnc_formats::convert::shp_frames_to_png(&shp, &pal) {
                        Ok(pngs) => {
                            let dir_default = derive_output_dir(path);
                            let dir = output.unwrap_or(&dir_default);
                            write_png_sequence(&pngs, dir, "frame")
                        }
                        Err(e) => {
                            report_convert_error(path, &e);
                            1
                        }
                    }
                }
                Err(e) => {
                    report_parse_error(path, &e);
                    1
                }
            }
        }
        Format::Pal => match cnc_formats::pal::Palette::parse(&data) {
            Ok(pal) => {
                eprintln!("Converting PAL → PNG (256-color swatch)...");
                match cnc_formats::convert::pal_to_png(&pal) {
                    Ok(png_data) => {
                        let out = output
                            .map(String::from)
                            .unwrap_or_else(|| derive_output(path, "png"));
                        write_single_file(&out, &png_data)
                    }
                    Err(e) => {
                        report_convert_error(path, &e);
                        1
                    }
                }
            }
            Err(e) => {
                report_parse_error(path, &e);
                1
            }
        },
        Format::Tmp => {
            let pal = require_palette(palette_path, "TMP (TD)");
            match cnc_formats::tmp::TdTmpFile::parse(&data) {
                Ok(tmp) => {
                    eprintln!("Converting TMP (TD) → PNG ({} tiles)...", tmp.tiles.len());
                    match cnc_formats::convert::td_tmp_tiles_to_png(&tmp, &pal) {
                        Ok(pngs) => {
                            let dir_default = derive_output_dir(path);
                            let dir = output.unwrap_or(&dir_default);
                            write_png_sequence(&pngs, dir, "tile")
                        }
                        Err(e) => {
                            report_convert_error(path, &e);
                            1
                        }
                    }
                }
                Err(e) => {
                    report_parse_error(path, &e);
                    1
                }
            }
        }
        Format::TmpRa => {
            let pal = require_palette(palette_path, "TMP (RA)");
            match cnc_formats::tmp::RaTmpFile::parse(&data) {
                Ok(tmp) => {
                    let present_count = tmp.tiles.iter().filter(|t| t.is_some()).count();
                    eprintln!("Converting TMP (RA) → PNG ({present_count} tiles)...");
                    match cnc_formats::convert::ra_tmp_tiles_to_png(&tmp, &pal) {
                        Ok(tiles) => {
                            let dir_default = derive_output_dir(path);
                            let dir = output.unwrap_or(&dir_default);
                            let present: Vec<_> = tiles.into_iter().flatten().collect();
                            write_png_sequence(&present, dir, "tile")
                        }
                        Err(e) => {
                            report_convert_error(path, &e);
                            1
                        }
                    }
                }
                Err(e) => {
                    report_parse_error(path, &e);
                    1
                }
            }
        }
        Format::Wsa => {
            let pal = require_palette(palette_path, "WSA");
            match cnc_formats::wsa::WsaFile::parse(&data) {
                Ok(wsa) => {
                    eprintln!(
                        "Converting WSA → PNG ({} frames, {}×{})...",
                        wsa.header.num_frames, wsa.header.width, wsa.header.height
                    );
                    match cnc_formats::convert::wsa_frames_to_png(&wsa, &pal) {
                        Ok(pngs) => {
                            let dir_default = derive_output_dir(path);
                            let dir = output.unwrap_or(&dir_default);
                            write_png_sequence(&pngs, dir, "frame")
                        }
                        Err(e) => {
                            report_convert_error(path, &e);
                            1
                        }
                    }
                }
                Err(e) => {
                    report_parse_error(path, &e);
                    1
                }
            }
        }
        Format::Fnt => match cnc_formats::fnt::FntFile::parse(&data) {
            Ok(fnt) => {
                eprintln!("Converting FNT → PNG (font atlas)...");
                match cnc_formats::convert::fnt_to_png(&fnt) {
                    Ok(png_data) => {
                        let out = output
                            .map(String::from)
                            .unwrap_or_else(|| derive_output(path, "png"));
                        write_single_file(&out, &png_data)
                    }
                    Err(e) => {
                        report_convert_error(path, &e);
                        1
                    }
                }
            }
            Err(e) => {
                report_parse_error(path, &e);
                1
            }
        },
        _ => {
            eprintln!("PNG conversion not supported for this format.");
            eprintln!("Supported: shp, pal, tmp, tmp-ra, wsa, fnt");
            1
        }
    }
}

/// Convert AUD to WAV.
#[cfg(feature = "convert")]
pub(super) fn convert_to_wav(path: &str, output: Option<&str>) -> i32 {
    let input = open_file(path);
    let mut aud = match cnc_formats::aud::AudStream::open(input) {
        Ok(aud) => aud,
        Err(e) => {
            report_parse_error(path, &e);
            return 1;
        }
    };
    eprintln!(
        "Converting AUD → WAV ({}Hz, {})...",
        aud.header().sample_rate,
        if aud.header().is_stereo() {
            "stereo"
        } else {
            "mono"
        }
    );

    let out = output
        .map(String::from)
        .unwrap_or_else(|| derive_output(path, "wav"));
    let output_file = match std::fs::File::create(&out) {
        Ok(file) => file,
        Err(e) => {
            eprintln!("Error writing {out}: {e}");
            return 1;
        }
    };

    match cnc_formats::convert::aud_stream_to_wav(&mut aud, output_file) {
        Ok(()) => match std::fs::metadata(&out) {
            Ok(meta) => {
                println!("Wrote {} bytes to {out}", meta.len());
                0
            }
            Err(_) => {
                println!("Wrote WAV to {out}");
                0
            }
        },
        Err(e) => {
            report_convert_error(path, &e);
            1
        }
    }
}

/// Convert SHP or WSA to animated GIF.
#[cfg(feature = "convert")]
pub(super) fn convert_to_gif(
    path: &str,
    explicit: Option<Format>,
    palette_path: Option<&str>,
    output: Option<&str>,
) -> i32 {
    let fmt = resolve_format(path, explicit);
    let data = read_file(path);

    // Default delay: 10 centiseconds = 100ms per frame ≈ 10 fps.
    let delay_cs: u16 = 10;

    match fmt {
        Format::Shp => {
            let pal = require_palette(palette_path, "SHP → GIF");
            match cnc_formats::shp::ShpFile::parse(&data) {
                Ok(shp) => {
                    eprintln!(
                        "Converting SHP → GIF ({} frames, {}×{})...",
                        shp.header.frame_count, shp.header.width, shp.header.height
                    );
                    match cnc_formats::convert::shp_frames_to_gif(&shp, &pal, delay_cs) {
                        Ok(gif_data) => {
                            let out = output
                                .map(String::from)
                                .unwrap_or_else(|| derive_output(path, "gif"));
                            write_single_file(&out, &gif_data)
                        }
                        Err(e) => {
                            report_convert_error(path, &e);
                            1
                        }
                    }
                }
                Err(e) => {
                    report_parse_error(path, &e);
                    1
                }
            }
        }
        Format::Wsa => {
            let pal = require_palette(palette_path, "WSA → GIF");
            match cnc_formats::wsa::WsaFile::parse(&data) {
                Ok(wsa) => {
                    eprintln!(
                        "Converting WSA → GIF ({} frames, {}×{})...",
                        wsa.header.num_frames, wsa.header.width, wsa.header.height
                    );
                    match cnc_formats::convert::wsa_frames_to_gif(&wsa, &pal, delay_cs) {
                        Ok(gif_data) => {
                            let out = output
                                .map(String::from)
                                .unwrap_or_else(|| derive_output(path, "gif"));
                            write_single_file(&out, &gif_data)
                        }
                        Err(e) => {
                            report_convert_error(path, &e);
                            1
                        }
                    }
                }
                Err(e) => {
                    report_parse_error(path, &e);
                    1
                }
            }
        }
        _ => {
            eprintln!("GIF conversion not supported for this format.");
            eprintln!("Supported: shp, wsa");
            1
        }
    }
}
