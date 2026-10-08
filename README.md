# PromisedFile

**Transfer virtual files without staging them as temporary files.**

PromisedFile is a Rust library for exposing data that does not yet exist as a regular file—such as archive entries, generated content, or remote objects—to other applications as native, transferable files.

On Windows, it uses Shell virtual-file formats and COM streams to provide file contents **on demand** through drag-and-drop or the clipboard. The receiving application can create the destination file without PromisedFile first writing an intermediate file to disk.

> **Project status:** Early development. Windows is the only implemented platform. The public API may change before a stable release.

## Why PromisedFile?

Consider an application that stores files inside an archive or generates a report on demand. A traditional *export-and-drag* approach might:

1. Extract or generate the data into a temporary file.
2. Pass that file path to the target application.
3. Clean up the temporary file later.

PromisedFile instead lets the application describe a virtual file and provide a reader for its contents:

```
ZIP entry / generated data / remote object
                    |
                FileSource
                    |
             VirtualFile(s)
                    |
        Windows IDataObject / IStream
                    |
           Explorer or another target
```

Content is read when the receiving application requests it. This avoids *library-managed staging files* and their associated disk writes and cleanup. The receiving application may still create the final destination file, as expected.

PromisedFile is **not** a virtual filesystem, a global drag-and-drop hook, or a replacement for an application's existing UI and drag lifecycle.

## Current features

- Windows virtual-file drag-and-drop and clipboard transfer.
- Single-file and multi-file transfers.
- On-demand content delivery through `IStream`.
- `FileSource` abstraction for supplying a fresh reader per request.
- `SourceReader::seekable(...)` and `SourceReader::streaming(...)`.
- Known file sizes and size derivation for seekable readers.
- `FactorySource` for adapting a closure without defining a new source type.
- Windows-specific `IDataObject` creation for integration with an application's own drag-and-drop handling.
- A working example that transfers multiple ZIP entries with lazy decompression.

## Getting started

### Requirements

- Windows and a Rust toolchain with the MSVC target.
- An application with a Windows message loop to initiate an interactive drag.

For local development, add the library to your application's `Cargo.toml`:

```toml
[dependencies]
promised-file = { path = "../promised-file" }
```

### Create a virtual file

```rust
use promised_file::{begin_drag, FactorySource, SourceReader, VirtualFile};
use std::io::Cursor;

fn start_drag() -> Result<(), Box<dyn std::error::Error>> {
    let source = FactorySource::new(|| {
        Ok(SourceReader::seekable(Cursor::new(b"hello".to_vec())))
    });

    let file = VirtualFile::new("hello.txt", Box::new(source))?
        .with_size(5);

    begin_drag(vec![file])?;
    Ok(())
}
```

Call `start_drag()` from an appropriate mouse interaction in your application's existing window procedure (for example, `WM_LBUTTONDOWN`). It is **not** intended to be called from a standalone console `main()` without a normal Windows drag interaction.

Pass multiple `VirtualFile` values to `begin_drag(vec![file_a, file_b])` to transfer more than one file.

### Choose a reader capability

`FileSource::open()` returns a new `SourceReader` whenever the receiving application requests a file's contents:

```rust
pub trait FileSource {
    fn open(&self) -> std::io::Result<SourceReader>;
}
```

Choose the appropriate reader when implementing a source:

```rust
SourceReader::seekable(reader)  // reader implements Read + Seek
SourceReader::streaming(reader) // reader implements Read
```

A seekable reader can determine its own size when `.with_size(...)` is omitted. A streaming reader can still be used when its size is known; supply that size with `.with_size(...)`.

**Important:** A non-seekable reader with an unknown size is not reliably supported across receiving applications. In the current implementation, `IStream::Stat` cannot report a size for that combination and returns an error. Do not rely on that mode for production use.

### Integrate with an existing drag-and-drop implementation

The high-level `begin_drag(...)` helper controls the drag operation for convenience. Applications that already manage `DoDragDrop`, their own `IDropSource`, allowed effects, or other drag behavior can instead obtain the Windows data object:

```rust
let data_object = promised_file::windows::create_data_object(files)?;
// Use the IDataObject in your application's existing OLE drag-and-drop flow.
```

This lower-level API is Windows-specific. The caller is responsible for the relevant COM/OLE initialization, thread and lifetime rules, and drag-operation management.

### Clipboard

The same virtual-file representation also supports Windows clipboard transfers. As with drag-and-drop, the source application must remain available to supply data while the receiving application requests it. The clipboard API and its session-lifetime contract are still being refined; a dedicated public usage example is planned.

## Examples

From the repository root:

```powershell
cargo test
cargo run --example simple_drag
cargo run --example zip_drag
```

For `zip_drag`, place a `sample.zip` in the working directory containing `hello.txt` and `world.txt` (or adjust the paths in the example). Drag from the example window into Explorer to verify that both extracted files have the expected contents.

The ZIP example lives outside the library core. It uses a ZIP reader adapter whose archive and entry remain alive while the target reads. **The ZIP entries are not fully decompressed into temporary files or a `Vec<u8>` before transfer.** Archive metadata is read first; entry contents are decompressed incrementally when requested.

ZIP and HTTP providers are **not built-in public adapters**. Applications can supply their own `FileSource` implementations, and future adapters may be developed separately.

## How it works on Windows

PromisedFile implements an OLE `IDataObject` that advertises:

- `CFSTR_FILEDESCRIPTORW`: names and available metadata for the virtual files.
- `CFSTR_FILECONTENTS`: a file's content, returned as `TYMED_ISTREAM` for the requested `lindex`.

Each request for content opens a reader for the corresponding `VirtualFile`. This allows multiple files and repeated content requests without requiring an up-front filesystem export.

The library's responsibilities stop at representing and delivering the virtual files. It does not intercept drag-and-drop globally or require applications to surrender control of their complete drag-and-drop workflow.

## Limitations

- **Windows-only** at present; cross-platform support is a goal, not a current feature.
- **Early-stage compatibility:** Successful Explorer transfers have been tested, but compatibility across other drag-and-drop and clipboard consumers has not been comprehensively validated.
- **Unknown-length streaming:** Non-seekable readers without a known size have limited interoperability, especially with consumers that query `IStream::Stat` or require seeking.
- **Incomplete `IStream` surface:** Some optional COM stream operations, such as `Clone` and `CopyTo`, are not yet implemented.
- **Synchronous reads:** Content providers currently expose synchronous Rust `Read` behavior; asynchronous and remote-I/O integration need further design.
- **Source lifetime:** The providing process must stay alive while lazy data is being requested. Source implementations are responsible for reopening their data and managing their own external resources.
- **No blanket zero-copy guarantee:** PromisedFile avoids staging files, but individual providers may buffer, allocate, or cache content.

## Roadmap

These are directions for future work, not promises of release dates.

- [ ] Harden Windows COM contracts, error handling, validation, and ownership/lifetime behavior.
- [ ] Test more Windows targets and large-file, multi-file, cancellation, and repeated-request scenarios.
- [ ] Refine the public Windows `IDataObject` integration API without taking over an application's drag lifecycle.
- [ ] Document and stabilize clipboard session behavior.
- [ ] Define an explicit strategy for unknown-size, non-seekable streams (including optional buffering/spooling if appropriate).
- [ ] Improve asynchronous and remote-source interoperability.
- [ ] Provide a C ABI, then optional C++ and C# bindings.
- [ ] Explore a macOS backend using native file-promise mechanisms.
- [ ] Expand examples and documentation; consider separate ZIP/HTTP adapters where useful.

## Design principles

1. **No mandatory temporary-file staging.** Prefer demand-driven streaming into the receiving application.
2. **Keep sources independent of transfer mechanics.** A `FileSource` supplies bytes; platform backends expose them as transferable files.
3. **Do not take over the host application's drag-and-drop design.** Offer both convenience helpers and lower-level integration points.
4. **Keep the core focused.** Archive formats, HTTP clients, cloud SDKs, and UI frameworks belong in examples or optional adapters, not mandatory core dependencies.
5. **Be explicit about limitations.** File-size knowledge, seekability, consumer compatibility, and process lifetime affect correctness.

## Contributing

The project is experimental and evolving. Bug reports, compatibility findings, and focused improvements are welcome. When reporting a transfer issue, include the receiving application, whether the source is seekable, whether its size is known, and the expected versus actual behavior.
