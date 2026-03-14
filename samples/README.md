# Sample Files

This directory holds real game file samples for manual testing and validation.

**These files are NOT tracked by git** (the `samples/` directory is in `.gitignore`).

## Expected structure

Place files in subdirectories matching their format:

```
samples/
  pal/        — .pal palette files
  mix/        — .mix archive files
  shp/        — .shp sprite files
  tmp/        — .tmp tile files
  aud/        — .aud audio files
  vqa/        — .vqa video files
  fnt/        — .fnt font files
  wsa/        — .wsa animation files
  ini/        — .ini configuration files
  mid/        — .mid MIDI files
  xmi/        — .xmi MIDI files
  wav/        — .wav audio files (for transcribe testing)
  adl/        — .adl Westwood ADL music files
```

## Sources

Typically extracted from:
- Tiberian Dawn (C&C1)
- Red Alert (C&C:RA)
- Tiberian Sun
- Red Alert 2

Use `cnc-formats extract` or a MIX editor to pull files from game archives.
