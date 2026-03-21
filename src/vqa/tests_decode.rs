// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2025–present Iron Curtain contributors

use super::tests::write_u16_le;
use super::*;

#[test]
fn palette_6bit_to_8bit_scaling() {
    let mut vqhd = [0u8; 42];
    write_u16_le(&mut vqhd, 0, 2);
    write_u16_le(&mut vqhd, 4, 1);
    write_u16_le(&mut vqhd, 6, 4);
    write_u16_le(&mut vqhd, 8, 2);
    vqhd[10] = 4;
    vqhd[11] = 2;
    vqhd[12] = 15;
    vqhd[13] = 1;
    write_u16_le(&mut vqhd, 16, 1);

    let mut cpl_data = vec![0u8; 768];
    cpl_data[3] = 63;
    cpl_data[4] = 63;
    cpl_data[5] = 63;

    let cbf_data = vec![0u8; 8];
    let vpt_data = vec![0u8; 2];

    let mut vqfr_payload = Vec::new();
    vqfr_payload.extend_from_slice(b"CPL0");
    vqfr_payload.extend_from_slice(&(cpl_data.len() as u32).to_be_bytes());
    vqfr_payload.extend_from_slice(&cpl_data);
    vqfr_payload.extend_from_slice(b"CBF0");
    vqfr_payload.extend_from_slice(&(cbf_data.len() as u32).to_be_bytes());
    vqfr_payload.extend_from_slice(&cbf_data);
    vqfr_payload.extend_from_slice(b"VPT0");
    vqfr_payload.extend_from_slice(&(vpt_data.len() as u32).to_be_bytes());
    vqfr_payload.extend_from_slice(&vpt_data);

    let vqhd_chunk_size = vqhd.len();
    let vqfr_chunk_size = vqfr_payload.len();
    let form_data_size = 4 + 8 + vqhd_chunk_size + 8 + vqfr_chunk_size;

    let mut data = Vec::new();
    data.extend_from_slice(b"FORM");
    data.extend_from_slice(&(form_data_size as u32).to_be_bytes());
    data.extend_from_slice(b"WVQA");
    data.extend_from_slice(b"VQHD");
    data.extend_from_slice(&(vqhd_chunk_size as u32).to_be_bytes());
    data.extend_from_slice(&vqhd);
    data.extend_from_slice(b"VQFR");
    data.extend_from_slice(&(vqfr_chunk_size as u32).to_be_bytes());
    data.extend_from_slice(&vqfr_payload);

    let vqa = VqaFile::parse(&data).unwrap();
    let frames = vqa.decode_frames().unwrap();
    assert_eq!(frames.len(), 1);

    let pal = &frames[0].palette;
    assert_eq!(pal[0], 0);
    assert_eq!(pal[1], 0);
    assert_eq!(pal[2], 0);
    assert_eq!(pal[3], 255);
    assert_eq!(pal[4], 255);
    assert_eq!(pal[5], 255);
}

#[test]
fn cbp_codebook_deferred_to_next_group() {
    let mut vqhd = [0u8; 42];
    write_u16_le(&mut vqhd, 0, 2);
    write_u16_le(&mut vqhd, 4, 2);
    write_u16_le(&mut vqhd, 6, 4);
    write_u16_le(&mut vqhd, 8, 2);
    vqhd[10] = 4;
    vqhd[11] = 2;
    vqhd[12] = 15;
    vqhd[13] = 1;
    write_u16_le(&mut vqhd, 16, 1);

    let cb_a = vec![0x01u8; 8];
    let cb_b = vec![0x02u8; 8];
    let vpt = vec![0u8; 2];

    let mut vqfr0 = Vec::new();
    vqfr0.extend_from_slice(b"CBF0");
    vqfr0.extend_from_slice(&(cb_a.len() as u32).to_be_bytes());
    vqfr0.extend_from_slice(&cb_a);
    vqfr0.extend_from_slice(b"VPT0");
    vqfr0.extend_from_slice(&(vpt.len() as u32).to_be_bytes());
    vqfr0.extend_from_slice(&vpt);

    let mut vqfr1 = Vec::new();
    vqfr1.extend_from_slice(b"CBP0");
    vqfr1.extend_from_slice(&(cb_b.len() as u32).to_be_bytes());
    vqfr1.extend_from_slice(&cb_b);
    vqfr1.extend_from_slice(b"VPT0");
    vqfr1.extend_from_slice(&(vpt.len() as u32).to_be_bytes());
    vqfr1.extend_from_slice(&vpt);

    let cpl = vec![0u8; 768];
    let mut cpl_chunk = Vec::new();
    cpl_chunk.extend_from_slice(b"CPL0");
    cpl_chunk.extend_from_slice(&(cpl.len() as u32).to_be_bytes());
    cpl_chunk.extend_from_slice(&cpl);

    let mut vqfr0_with_cpl = cpl_chunk;
    vqfr0_with_cpl.extend_from_slice(&vqfr0);

    let chunks_size = (8 + vqfr0_with_cpl.len()) + (8 + vqfr1.len());
    let form_data_size = 4 + 8 + 42 + chunks_size;
    let mut data = Vec::new();
    data.extend_from_slice(b"FORM");
    data.extend_from_slice(&(form_data_size as u32).to_be_bytes());
    data.extend_from_slice(b"WVQA");
    data.extend_from_slice(b"VQHD");
    data.extend_from_slice(&42u32.to_be_bytes());
    data.extend_from_slice(&vqhd);
    data.extend_from_slice(b"VQFR");
    data.extend_from_slice(&(vqfr0_with_cpl.len() as u32).to_be_bytes());
    data.extend_from_slice(&vqfr0_with_cpl);
    data.extend_from_slice(b"VQFR");
    data.extend_from_slice(&(vqfr1.len() as u32).to_be_bytes());
    data.extend_from_slice(&vqfr1);

    let vqa = VqaFile::parse(&data).unwrap();
    let frames = vqa.decode_frames().unwrap();
    assert_eq!(frames.len(), 2);
    assert!(frames[0].pixels.iter().all(|&p| p == 0x01));
    assert!(frames[1].pixels.iter().all(|&p| p == 0x01));
}
