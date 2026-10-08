# promised-file

Expose data as a file on Windows without writing it to a temporary file first.

`promised-file` is a Rust library for Windows OLE virtual-file transfer. It lets an application offer data from memory, an archive, a generated report, or another source as files through drag-and-drop or the clipboard. The receiving application asks for the contents when it needs them.

The project is under development. Only Windows is implemented, and the API is not stable yet.

## How it works

Normally, exporting a file that only exists inside an application means writing it somewhere first, passing the path to another application, and cleaning it up afterward. This library supplies a stream instead of a temporary path.

On Windows, an `IDataObject` advertises `CFSTR_FILEDESCRIPTORW` (file names and metadata) and `CFSTR_FILECONTENTS` (file contents as an `IStream`). For multiple files, the receiving application selects the content by `FORMATETC::lindex`.

```text
VirtualFile(s)  [name, optional size, FileSource]
    │
    ▼
IDataObject  ──► CFSTR_FILEDESCRIPTORW (metadata)
    │
    └── GetData(CFSTR_FILECONTENTS, lindex)
            │
            ▼
        FileSource::open()
            │
            ▼
        SourceReader ──► IStream::Read() ──► Receiving application
```

The destination application may of course create a real file. What is avoided is *staging* that file on disk in the source application.

This is not a virtual filesystem or a global drag-and-drop hook.

## Using it

For local development:

```toml
[dependencies]
promised-file = { path = "../promised-file" }
```

A `VirtualFile` has a name, optional size, and a `FileSource`. A source must return a **new reader** each time `open()` is called; the receiving application is allowed to request the contents more than once.

```rust
use promised_file::{begin_drag, FactorySource, SourceReader, VirtualFile};
use std::io::Cursor;

fn start_drag() -> promised_file::Result<()> {
    let source = FactorySource::new(|| {
        Ok(SourceReader::seekable(Cursor::new(b"hello\n".to_vec())))
    });

    let file = VirtualFile::new("hello.txt", Box::new(source))?;
    begin_drag(vec![file])?;
    Ok(())
}
```

Call `start_drag()` from an appropriate mouse event in an application with a Windows message loop, not from a bare console `main()`. See `examples/simple_drag.rs` for the complete window setup.

Pass a `Vec<VirtualFile>` to transfer multiple files. The same underlying representation is used for clipboard transfers.

### Readers and file sizes

`FileSource` is intentionally small:

```rust
pub trait FileSource {
    fn open(&self) -> std::io::Result<SourceReader>;
}
```

There are two kinds of readers:

```rust
SourceReader::seekable(reader)  // Read + Seek
SourceReader::streaming(reader) // Read only
```

If the size is already known, set it with `.with_size(size)`. If it isn't specified, a seekable reader can determine its length without changing its current position. A streaming reader can work without `Seek` as long as the file size is provided.

**Known limitation:** a non-seekable reader with an unknown size cannot currently provide a meaningful `IStream::Stat::cbSize`. The current `Stat` implementation returns an error in that case. Support across receiving applications is therefore not guaranteed.

### Use your own drag-and-drop flow

`begin_drag()` is a convenience API. It isn't meant to own every application's drag-and-drop behavior.

If the application already has its own `IDropSource`, drag effects, or `DoDragDrop` call, it can create just the data object:

```rust
let data_object = promised_file::windows::create_data_object(files)?;
// Use this IDataObject with your existing OLE drag-and-drop code.
```

The caller of this lower-level API is responsible for appropriate OLE/COM initialization, threading, and lifetime management.

## Code layout

```text
src/
├── lib.rs                 Public API / re-exports
├── virtual_file.rs        Name, size metadata, and source of one virtual file
├── file_source.rs         FileSource trait
├── source_reader.rs       Streaming / Seekable reader and size derivation
├── factory_source.rs      Closure-based FileSource implementation
├── error.rs               Public error types
├── windows.rs             Windows module entry point and OLE initialization
└── windows/
    ├── data_object.rs     IDataObject; dispatch by format and file index
    ├── formats.rs         File descriptor formats and STGMEDIUM construction
    ├── stream.rs          IStream adapter around SourceReader
    ├── drop_source.rs     IDropSource for the convenience drag operation
    ├── drag_drop.rs       DoDragDrop integration
    ├── clipboard.rs       OLE clipboard integration
    └── error.rs           Windows error conversion

examples/
├── simple_drag.rs         Drag a file backed by in-memory data
└── zip_drag.rs            Drag multiple ZIP entries without extracting first
```

The division is deliberate: `VirtualFile`, `FileSource`, and `SourceReader` don't know anything about Win32. The Windows backend translates them into the COM interfaces understood by the Shell. `FactorySource` is only a convenience adapter; users can implement `FileSource` directly.

In the Windows backend, `IDataObject::GetData` returns either the group of file descriptors or a new `IStream` for the requested file index. `IStream::Read` forwards to the source reader. `IStream::Seek` is available only when the source reader supports it.

## Examples

```powershell
cargo test
cargo run --example simple_drag
cargo run --example zip_drag
```

Both drag examples open a window. Press and hold the left mouse button in that window, then drop into Explorer.

The ZIP example currently expects `sample.zip` in the working directory containing `hello.txt` and `world.txt`. When run from the repository root with `cargo run`, put the ZIP there (or change the paths in the example).

The ZIP implementation lives **only in the example**. It reads entry metadata first, but it does not extract the file contents to a `Vec<u8>` or a temporary file before transfer. `LazyZipEntryReader` keeps the archive and ZIP entry alive while `Read` decompresses data on demand. Its ZIP-specific dependencies are not part of the library's core API.

## Current state and limitations

Working paths have been exercised with Windows Explorer: single and multiple virtual files, clipboard transfer, seekable and streaming readers with known sizes, and seekable readers with derived sizes. The ZIP example exercises lazy decompression through the same interfaces.

This is not yet a compatibility-tested replacement for every Windows drag-and-drop implementation. In particular:

- Behavior with targets other than Explorer needs broader testing.
- Unknown-size, non-seekable streams are limited as described above.
- Some `IStream` methods, including `CopyTo` and `Clone`, are not implemented.
- Large transfers, cancellation, error propagation, and source-process lifetime need more testing.
- Reads are synchronous. A source backed by network I/O may block while supplying data.

## Next steps

Near term: improve COM compatibility tests, harden error handling and file-name validation, document clipboard lifetime behavior, and refine the lower-level `IDataObject` API so applications can retain control of their own drag-and-drop flow.

Later: a C ABI and optional C++/C# wrappers, a strategy for unknown-length streaming, and a macOS backend. ZIP and HTTP integrations are candidates for separate adapters rather than required dependencies of the core crate.

There is no release schedule for these items.
