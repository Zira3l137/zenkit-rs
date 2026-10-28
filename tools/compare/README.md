# Comparing against ZenKit

These tools check zenkit-rs against ZenKit on a real Gothic installation. Each side dumps every
asset of one kind as JSON lines, and `compare.py` diffs the two.

| Kind  | Files                          |
|-------|--------------------------------|
| `mrm` | Multi-resolution meshes (props) |
| `msh` | Static meshes                   |
| `tex` | Textures                        |
| `mdh` | Skeletons                       |
| `mdm` | Model meshes                    |
| `mdl` | Models (skeleton and meshes)    |
| `mmb` | Morph meshes                    |
| `man` | Animations                      |

## Setup

```nu
pip install zenkit
```

## Running

In Nushell, from the repository root, with `$game` set to the Gothic directory (the one containing
`Data/`):

```nu
let game = 'C:\Games\Gothic II'
cargo run --release -p zk-dump -- mrm $game rs.jsonl
python tools/compare/dump_zenkit.py mrm $game zk.jsonl
python tools/compare/compare.py rs.jsonl zk.jsonl
```

`zk-dump` takes seconds. The Python side goes through ctypes for every single value, so expect
minutes for a few thousand files. If one file takes over a minute, it prints the file's path and
where it is stuck, and keeps going.

## Extracting a file

To look at a file that fails, or attach it to a bug report, copy it out of the archives. Either the
full path from the comparison output or just the file name works:

```nu
cargo run --release -p zk-dump -- extract $game _work/data/anims/_compiled/door.mdl door.mdl
```

## Reading the result

- **identical** / **differ**: files whose parsed contents match or not.
- **Known, intended differences**: places where zenkit-rs fixes a ZenKit bug, listed in `KNOWN`
  at the top of `compare.py`.
- **Values ZenKit's Python bindings could not read**: usually enum values the bindings reject.
- **Unexpected differences**: everything else, with a few example files per field. These are what
  to look at.

Animation events and the sample position range have no getter in ZenKit's C API, so they show up
under "could not read" and are only checked on the zenkit-rs side.

Texture pixels are compared as SHA-256 hashes of the decoded RGBA data. Formats ZenKit cannot
decode (P8, A1R5G5B5, DXT2, DXT4) are only checked on the zenkit-rs side.
