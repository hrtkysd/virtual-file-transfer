# virtual-file-transfer

Expose data as a file on Windows without writing it to a temporary file first.

`virtual-file-transfer` is a Rust library for Windows OLE virtual-file transfer. It lets an application offer data from memory, an archive, a generated report, or another source as files through drag-and-drop or the clipboard. The receiving application asks for the contents when it needs them.

The project is under development. Only Windows is implemented, and the API is not stable yet.

## How it works

Normally, exporting a file that only exists inside an application means writing it somewhere first, passing the path to another application, and cleaning it up afterward. This library supplies a stream instead of a temporary path.

On Windows, an `IDataObject` advertises `CFSTR_FILEDESCRIPTORW` (file names and metadata) and `CFSTR_FILECONTENTS` (file contents as an `IStream`). For multiple files, the receiving application selects the content by `FORMATETC::lindex`.

```text
VirtualFile(s) [name, optional size, FileSource]
    │
    ▼
IDataObject ──► CFSTR_FILEDESCRIPTORW (metadata)
    │
    └── GetData(CFSTR_FILECONTENTS, lindex)
            │
            ▼
        FileSource::open()
            │
            ▼
        SourceReader ──► IStream::Read() ──► Receiving application
```

The destination application may of course create a real file. What is avoided is **staging** that file on disk in the source application.

This is not a virtual filesystem or a global drag-and-drop hook.

## Using it

For local development:

```toml
[dependencies]
virtual-file-transfer = { path = "../virtual-file-transfer" }
```

To enable explicit spooling support:

```toml
[dependencies]
virtual-file-transfer = { path = "../virtual-file-transfer", features = ["spool"] }
```

A `VirtualFile` has a name, optional size, and a `FileSource`. A source must return a **new reader** each time `open()` is called; the receiving application is allowed to request the contents more than once.

```rust
use virtual_file_transfer::{begin_drag, FactorySource, SourceReader, VirtualFile};
use std::io::Cursor;

fn start_drag() -> virtual_file_transfer::Result<()> {
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

### Cancellation

Transfers can be cancelled using a `CancellationToken`.

```rust
use virtual_file_transfer::{begin_drag, TransferRequest};

let request = TransferRequest::new(files);
let token = request.cancellation_token();

// Share the token with another thread or event handler.
// It can request cancellation even while begin_drag() is running.
let handle = begin_drag(request)?;

// Elsewhere, when cancellation is requested:
// token.cancel();

// Alternatively, cancel an ongoing asynchronous transfer:
// handle.cancel();
```

Clipboard transfers can be cancelled through `ClipboardSession::cancel()`. Cancelling a clipboard session also affects subsequent paste operations using the same clipboard data.

Cancellation is cooperative. An in-progress blocking read is not forcibly interrupted; cancellation is observed when the read operation can check the token.

### Readers and file sizes

`FileSource` is intentionally small:

```rust
pub trait FileSource {
    fn open(&self) -> std::io::Result<SourceReader>;
}
```

There are two basic kinds of readers:

```rust
SourceReader::seekable(reader)  // Read + Seek
SourceReader::streaming(reader) // Read only
```

If the size is already known, set it with `.with_size(size)`. If it isn't specified, a seekable reader can determine its length without changing its current position.

A streaming reader can work without `Seek` as long as the file size is provided.

| Source capabilities | Direct transfer | `IStream::Stat` | `IStream::Seek` | Spooling required |
|---|---|---|---|---|
| Known size + seekable | Yes | Supported | Supported | No |
| Known size + non-seekable | Yes | Supported from supplied size | Not supported | No |
| Unknown size + seekable | Yes | Supported by deriving the size | Supported | No |
| Unknown size + non-seekable | Receiver-dependent | Size cannot be reported | Not supported | For receivers requiring size or seeking |

#### Unknown-size, non-seekable sources

A non-seekable reader whose size is unknown cannot provide a meaningful `IStream::Stat::cbSize` without first consuming or materializing the source.

The library does not silently materialize such streams.

With the current implementation, `SourceReader::streaming(reader)` remains a true sequential reader. If its size is not supplied separately, `IStream::Stat` returns an error when asked for the stream size.

The currently tested Windows Explorer transfer path calls `IStream::Stat` and requires the size to be available, so an unknown-size, non-seekable source cannot currently be transferred directly to Explorer.

If the size is available from source metadata, prefer supplying it explicitly:

```rust
let file = VirtualFile::new("data.bin", Box::new(source))?
    .with_size(size);
```

This preserves direct streaming and does not require staging the contents first.

#### Explicit spooling

For a source that is both non-seekable and of unknown size, applications can explicitly trade temporary storage for broader receiver compatibility by enabling the `spool` feature:

```toml
virtual-file-transfer = { version = "0.1", features = ["spool"] }
```

Then create a spooled reader explicitly:

```rust
let reader = SourceReader::spooled(source)?;
```

Spooling consumes the complete source into temporary storage before transfer and exposes the result as a seekable reader. Its size can then be derived in the same way as any other seekable reader.

| Reader | Temporary storage | Size available | Seekable |
|---|---|---|---|
| `SourceReader::streaming` | No | Only if supplied separately | No |
| `SourceReader::seekable` | No | Derivable if not supplied | Yes |
| `SourceReader::spooled` | Yes | Derivable | Yes |

Spooling is always explicit. `virtual-file-transfer` does not automatically switch a streaming source to temporary storage in response to receiver behavior.

### Use your own drag-and-drop flow

`begin_drag()` is a convenience API. It isn't meant to own every application's drag-and-drop behavior.

If the application already has its own `IDropSource`, drag effects, or `DoDragDrop` call, it can create just the data object:

```rust
let data_object = virtual_file_transfer::windows::create_data_object(files)?;
// Use this IDataObject with your existing OLE drag-and-drop code.
```

The caller of this lower-level API is responsible for appropriate OLE/COM initialization, threading, and lifetime management.

## Code layout

```
src/
├── lib.rs                     Public API / re-exports
├── virtual_file.rs            Name, size metadata, and source of one virtual file
├── file_source.rs             FileSource trait
├── source_reader.rs           Streaming / Seekable reader and size derivation
├── factory_source.rs          Closure-based FileSource implementation
├── transfer.rs                TransferRequest and transfer status
├── cancellation.rs            CancellationToken
├── error.rs                   Public error types
├── windows.rs                 Windows module entry point and OLE initialization
└── windows/
    ├── data_object.rs         IDataObject; dispatch by format and file index
    ├── data_object/
    │   └── async_operation.rs IDataObjectAsyncCapability and async operation lifecycle
    ├── formats.rs             File descriptor formats and STGMEDIUM construction
    ├── stream.rs              IStream adapter around SourceReader
    ├── drop_source.rs         IDropSource for the convenience drag operation
    ├── drag_drop.rs           DoDragDrop integration
    ├── clipboard.rs           OLE clipboard integration
    └── error.rs               Windows error conversion

examples/
├── simple_drag.rs             Drag a file backed by in-memory data
├── zip_drag.rs                Drag multiple ZIP entries without extracting first
├── slow_drag.rs               Slow drag transfer and cancellation testing
└── slow_clipboard.rs          Slow clipboard transfer and cancellation testing
```

The division is deliberate: `VirtualFile`, `FileSource`, and `SourceReader` don't know anything about Win32. The Windows backend translates them into the COM interfaces understood by the Shell. `FactorySource` is only a convenience adapter; users can implement `FileSource` directly.

In the Windows backend, `IDataObject::GetData` returns either the group of file descriptors or a new `IStream` for the requested file index. `IStream::Read` forwards to the source reader. `IStream::Seek` is available only when the source reader supports it.

## Examples

```powershell
cargo test
cargo run --example simple_drag
cargo run --example zip_drag
cargo run --example slow_drag
cargo run --example slow_clipboard
```

Both drag examples open a window. Press and hold the left mouse button in that window, then drop into Explorer.

The ZIP example currently expects `sample.zip` in the working directory containing `hello.txt` and `world.txt`. When run from the repository root with `cargo run`, put the ZIP there (or change the paths in the example).

The ZIP implementation lives **only in the example**. It reads entry metadata first, but it does not extract the file contents to a `Vec<u8>` or a temporary file before transfer. `LazyZipEntryReader` keeps the archive and ZIP entry alive while `Read` decompresses data on demand. Its ZIP-specific dependencies are not part of the library's core API.

This is also an example of a known-size, non-seekable source: the ZIP reader itself is sequential, but the uncompressed entry size is available from ZIP metadata and is supplied to `VirtualFile`.

## Current state and limitations

Working paths have been exercised with Windows Explorer: single and multiple virtual files, clipboard transfer, seekable and streaming readers with known sizes, and seekable readers with derived sizes. The ZIP example exercises lazy decompression through the same interfaces.

This is not yet a compatibility-tested replacement for every Windows drag-and-drop implementation. In particular:

- Behavior with targets other than Explorer needs broader testing.
- Unknown-size, non-seekable streams cannot currently be transferred to the tested Explorer path without first making the size available, for example through explicit spooling.
- Some `IStream` methods, including `CopyTo` and `Clone`, are not implemented.
- Cancellation has been tested with Windows Explorer for synchronous and asynchronous drag-and-drop transfers, as well as clipboard transfers. Other receiving applications need further testing.
- Large transfers, error propagation, and source-process lifetime need more testing.
- Individual `IStream::Read` calls are synchronous. A source backed by network I/O may block while supplying data, even during an asynchronous transfer.
- Explicit spooling trades temporary storage and startup latency for a determinable size and seekability.

## Next steps

Near term: improve COM compatibility tests, harden error handling and file-name validation, test explicit spooling with large and failing sources, document clipboard lifetime behavior, and refine the lower-level `IDataObject` API so applications can retain control of their own drag-and-drop flow.

Later: a C ABI and optional C++/C# wrappers, and a macOS backend. ZIP and HTTP integrations are candidates for separate adapters rather than required dependencies of the core crate.

There is no release schedule for these items.
