use cnc_formats::{aud, big, mix, vqa};

#[cfg(feature = "meg")]
use cnc_formats::meg;

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

struct CountingAllocator;

static ALLOC_ACTIVE: AtomicBool = AtomicBool::new(false);
static ALLOC_CALLS: AtomicUsize = AtomicUsize::new(0);
static ALLOC_LOCK: Mutex<()> = Mutex::new(());

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

// SAFETY: `GlobalAlloc` is an unsafe trait because callers rely on allocator
// implementations upholding the allocation contract. This test wrapper does
// not implement custom allocation logic; it only counts calls and forwards
// directly to `std::alloc::System` with the exact same pointer/layout values.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ALLOC_ACTIVE.load(Ordering::Relaxed) {
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if ALLOC_ACTIVE.load(Ordering::Relaxed) {
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if ALLOC_ACTIVE.load(Ordering::Relaxed) {
            ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[test]
fn hot_paths_do_not_heap_allocate_after_setup() {
    let _guard = ALLOC_LOCK.lock().unwrap();

    // On some Linux configurations the very first `measure_allocs` window
    // that executes real work picks up 1-2 allocations from lazy per-thread
    // runtime state (glibc arena init, TLS destructors, Rust test-harness
    // output capture buffers, etc.).  An empty closure is not enough to
    // trigger this — it only fires when code actually runs inside the
    // window.  Absorb that one-time cost with a throwaway measurement of
    // the same function, then assert on the second (steady-state) call.
    let _ = measure_allocs(|| mix::crc("TARGET.BIN").to_raw());

    let crc_allocs = measure_allocs(|| mix::crc("TARGET.BIN").to_raw());
    assert_eq!(crc_allocs, 0, "mix::crc allocated in the hot path");

    let mix_bytes = build_mix(&[("TARGET.BIN", &[0xAA; 64]), ("OTHER.BIN", &[0x55; 32])]);
    let archive = mix::MixArchive::parse(&mix_bytes).unwrap();
    let mix_key = mix::crc("TARGET.BIN");
    let lookup_allocs = measure_allocs(|| archive.get_by_crc(mix_key).map_or(0, <[u8]>::len));
    assert_eq!(
        lookup_allocs, 0,
        "MixArchive::get_by_crc allocated after parse"
    );

    let aud_bytes = aud::build_aud(&build_audio_samples(4096), 22_050, false);
    let mut aud_stream = aud::AudStream::open_seekable(Cursor::new(aud_bytes.as_slice())).unwrap();
    let mut aud_scratch = [0i16; 1024];
    let aud_allocs = measure_allocs(|| aud_stream.read_samples(&mut aud_scratch).unwrap());
    assert_eq!(
        aud_allocs, 0,
        "AudStream::read_samples allocated after open"
    );

    let vqa_stream_bytes = build_vqa_stream_probe();
    let mut vqa_stream = vqa::VqaStream::open(Cursor::new(vqa_stream_bytes.as_slice())).unwrap();
    let _ = vqa_stream.next_chunk().unwrap();
    let vqa_stream_allocs = measure_allocs(|| {
        vqa_stream
            .next_chunk()
            .unwrap()
            .map_or(0, |chunk| chunk.data.len())
    });
    assert_eq!(
        vqa_stream_allocs, 0,
        "VqaStream::next_chunk allocated after scratch warmup"
    );

    let vqa_bytes = build_vqa_with_audio();
    let mut decoder = vqa::VqaDecoder::open(Cursor::new(vqa_bytes.as_slice())).unwrap();
    let mut frame_buffer = vqa::VqaFrameBuffer::from_media_info(&decoder.media_info());
    let _ = decoder.next_frame_into(&mut frame_buffer).unwrap();
    let mut vqa_scratch = [0i16; 1024];
    let vqa_allocs = measure_allocs(|| decoder.read_audio_samples(&mut vqa_scratch).unwrap());
    assert_eq!(
        vqa_allocs, 0,
        "VqaDecoder::read_audio_samples allocated after queue priming"
    );

    // Archive streaming copy: copy_by_index uses a stack buffer, so zero
    // heap allocations are expected during the copy itself.
    let mix_stream_bytes = build_mix(&[("COPY.BIN", &[0xBB; 256])]);
    let mut archive = mix::MixArchiveReader::open(Cursor::new(mix_stream_bytes)).unwrap();
    // Prime the internal BufReader by performing one copy.
    let _ = archive.copy_by_index(0, &mut std::io::sink());
    // Measure the second copy (seeks back, reads, writes — all on warm buffers).
    let copy_allocs = measure_allocs(|| {
        let _ = archive.copy_by_index(0, &mut std::io::sink());
    });
    assert_eq!(
        copy_allocs, 0,
        "MixArchiveReader::copy_by_index allocated during streaming copy"
    );

    // BIG archive streaming copy: same zero-alloc guarantee.
    let big_bytes = build_big(&[("COPY.BIN", &[0xCC; 256])]);
    let mut big_archive = big::BigArchiveReader::open(Cursor::new(big_bytes)).unwrap();
    let _ = big_archive.copy_by_index(0, &mut std::io::sink());
    let big_copy_allocs = measure_allocs(|| {
        let _ = big_archive.copy_by_index(0, &mut std::io::sink());
    });
    assert_eq!(
        big_copy_allocs, 0,
        "BigArchiveReader::copy_by_index allocated during streaming copy"
    );

    // MEG archive streaming copy: same zero-alloc guarantee.
    #[cfg(feature = "meg")]
    {
        let meg_bytes = build_meg(&[("COPY.BIN", &[0xDD; 256])]);
        let mut meg_archive = meg::MegArchiveReader::open(Cursor::new(meg_bytes)).unwrap();
        let _ = meg_archive.copy_by_index(0, &mut std::io::sink());
        let meg_copy_allocs = measure_allocs(|| {
            let _ = meg_archive.copy_by_index(0, &mut std::io::sink());
        });
        assert_eq!(
            meg_copy_allocs, 0,
            "MegArchiveReader::copy_by_index allocated during streaming copy"
        );
    }
}

fn write_u32_be(buf: &mut Vec<u8>, value: u32) {
    buf.extend_from_slice(&value.to_be_bytes());
}

fn write_u16_le(buf: &mut [u8], offset: usize, value: u16) {
    if let Some(dst) = buf.get_mut(offset..offset.saturating_add(2)) {
        dst.copy_from_slice(&value.to_le_bytes());
    }
}

fn build_vqa_stream_probe() -> Vec<u8> {
    let mut vqhd = [0u8; 42];
    write_u16_le(&mut vqhd, 0, 2);
    write_u16_le(&mut vqhd, 4, 1);
    write_u16_le(&mut vqhd, 6, 4);
    write_u16_le(&mut vqhd, 8, 2);
    if let Some(slot) = vqhd.get_mut(10) {
        *slot = 4;
    }
    if let Some(slot) = vqhd.get_mut(11) {
        *slot = 2;
    }
    if let Some(slot) = vqhd.get_mut(12) {
        *slot = 15;
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"FORM");
    let form_size = 4usize + (8 + vqhd.len()) + (8 + 8);
    write_u32_be(&mut out, form_size as u32);
    out.extend_from_slice(b"WVQA");
    out.extend_from_slice(b"VQHD");
    write_u32_be(&mut out, vqhd.len() as u32);
    out.extend_from_slice(&vqhd);
    out.extend_from_slice(b"JUNK");
    write_u32_be(&mut out, 8);
    out.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    out
}

fn measure_allocs<F, T>(f: F) -> usize
where
    F: FnOnce() -> T,
{
    ALLOC_CALLS.store(0, Ordering::SeqCst);
    ALLOC_ACTIVE.store(true, Ordering::SeqCst);
    let _ = f();
    ALLOC_ACTIVE.store(false, Ordering::SeqCst);
    ALLOC_CALLS.load(Ordering::SeqCst)
}

fn build_mix(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut entries: Vec<(mix::MixCrc, &[u8])> = files
        .iter()
        .map(|(name, data)| (mix::crc(name), *data))
        .collect();
    entries.sort_by_key(|(crc, _)| crc.to_raw() as i32);

    let count = entries.len() as u16;
    let mut offsets = Vec::with_capacity(entries.len());
    let mut current = 0u32;
    for (_, data) in &entries {
        offsets.push(current);
        current = current.saturating_add(data.len() as u32);
    }

    let mut out = Vec::new();
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&current.to_le_bytes());
    for (index, (crc, data)) in entries.iter().enumerate() {
        out.extend_from_slice(&crc.to_raw().to_le_bytes());
        out.extend_from_slice(&offsets[index].to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    }
    for (_, data) in &entries {
        out.extend_from_slice(data);
    }
    out
}

fn build_big(files: &[(&str, &[u8])]) -> Vec<u8> {
    let table_size: usize = files.iter().map(|(name, _)| 8 + name.len() + 1).sum();
    let data_start = 16 + table_size;
    let archive_size = data_start + files.iter().map(|(_, data)| data.len()).sum::<usize>();

    let mut out = Vec::with_capacity(archive_size);
    out.extend_from_slice(b"BIGF");
    out.extend_from_slice(&(archive_size as u32).to_le_bytes());
    out.extend_from_slice(&(files.len() as u32).to_be_bytes());
    out.extend_from_slice(&(data_start as u32).to_be_bytes());

    let mut offset = data_start as u32;
    for (name, data) in files {
        out.extend_from_slice(&offset.to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        offset = offset.saturating_add(data.len() as u32);
    }

    for (_, data) in files {
        out.extend_from_slice(data);
    }
    out
}

#[cfg(feature = "meg")]
fn build_meg(files: &[(&str, &[u8])]) -> Vec<u8> {
    let count = files.len() as u32;
    let mut buf = Vec::new();

    buf.extend_from_slice(&count.to_le_bytes());
    buf.extend_from_slice(&count.to_le_bytes());

    for (name, _) in files {
        let name_len = name.len() as u16;
        buf.extend_from_slice(&name_len.to_le_bytes());
        buf.extend_from_slice(name.as_bytes());
    }

    let records_total = files.len() * 20;
    let data_start = buf.len() + records_total;

    let mut offset = data_start;
    let mut offsets = Vec::with_capacity(files.len());
    for (_, data) in files {
        offsets.push(offset as u32);
        offset += data.len();
    }

    for (index, (_, data)) in files.iter().enumerate() {
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&(index as u32).to_le_bytes());
        buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
        buf.extend_from_slice(&offsets[index].to_le_bytes());
        buf.extend_from_slice(&(index as u32).to_le_bytes());
    }

    for (_, data) in files {
        buf.extend_from_slice(data);
    }
    buf
}

fn build_audio_samples(sample_frames: usize) -> Vec<i16> {
    let mut samples = Vec::with_capacity(sample_frames);
    for index in 0..sample_frames {
        let centered = ((index * 17) % 257) as i32 - 128;
        samples.push((centered * 192) as i16);
    }
    samples
}

fn build_vqa_with_audio() -> Vec<u8> {
    let width = 16u16;
    let height = 8u16;
    let frames = vec![
        vec![0x11; usize::from(width) * usize::from(height)],
        vec![0x44; usize::from(width) * usize::from(height)],
    ];
    let mut palette = [0u8; 768];
    for index in 0..256usize {
        let base = index * 3;
        palette[base] = index as u8;
        palette[base + 1] = 255u8.saturating_sub(index as u8);
        palette[base + 2] = ((index * 5) & 0xFF) as u8;
    }

    let audio_samples = build_audio_samples(2940);
    let audio = vqa::VqaAudioInput {
        samples: &audio_samples,
        sample_rate: 22_050,
        channels: 1,
    };
    let params = vqa::VqaEncodeParams {
        fps: 15,
        ..vqa::VqaEncodeParams::default()
    };

    vqa::encode_vqa(&frames, &palette, width, height, Some(&audio), &params).unwrap()
}
