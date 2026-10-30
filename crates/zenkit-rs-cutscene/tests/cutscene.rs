use std::path::PathBuf;

use zenkit_rs_cutscene::ConversationMessage;
use zenkit_rs_cutscene::CutsceneLibrary;
use zenkit_rs_cutscene::MessageState;

/// What a library holds, in a form both encodings can be written from.
enum Block<'a> {
    Line {
        name: &'a str,
        text: &'a [u8],
        wav: &'a str,
    },
    /// A block holding a block holding the line.
    Nested {
        name: &'a str,
        text: &'a [u8],
        wav: &'a str,
    },
    /// A block whose atomic block holds a null message.
    Empty {
        name: &'a str,
    },
    /// A block with two sub-blocks, which nobody knows how to read.
    Several {
        name: &'a str,
    },
}

const BLOCKS: &[Block] = &[
    Block::Line { name: "DIA_B_01", text: b"Gr\xFC\xDF dich.", wav: "DIA_B_01.WAV" },
    Block::Nested { name: "dia_a_01", text: b"Hallo.", wav: "DIA_A_01.WAV" },
    Block::Empty { name: "DIA_C_01" },
    Block::Several { name: "DIA_D_01" },
    Block::Line { name: "DIA_E_01", text: b"", wav: "DIA_E_01.WAV" },
];

fn ascii(save: bool) -> Vec<u8> {
    let mut out = format!(
        "ZenGin Archive\nver 1\nzCArchiverGeneric\nASCII\nsaveGame {}\nEND\nobjects 9\nEND\n\n\
         [% zCCSLib 0 0]\n\tNumOfItems=int:{}\n",
        save as u8,
        BLOCKS.len()
    )
    .into_bytes();
    let mut push = |s: &[u8]| out.extend_from_slice(s);
    let message = |push: &mut dyn FnMut(&[u8]), text: &[u8], wav: &str| {
        push(b"[% oCMsgConversation:oCNpcMessage:zCEventMessage 0 3]\nsubType=enum:0\n");
        if save {
            push(b"highPriority=bool:1\ninUse=bool:0\ndeleted=bool:1\n");
        }
        push(b"text=string:");
        push(text);
        push(format!("\nname=string:{wav}\n[]\n").as_bytes());
    };
    let header = |push: &mut dyn FnMut(&[u8]), name: &str, count: i32| {
        push(
            format!("blockName=string:{name}\nnumOfBlocks=int:{count}\nsubBlock0=float:0\n")
                .as_bytes(),
        );
    };

    for block in BLOCKS {
        push(b"\t[% zCCSBlock 0 1]\n");
        match *block {
            Block::Line { name, text, wav } => {
                header(&mut push, name, 1);
                push(b"[% zCCSAtomicBlock 0 2]\n");
                message(&mut push, text, wav);
                push(b"[]\n");
            }
            Block::Nested { name, text, wav } => {
                header(&mut push, name, 1);
                push(b"[% zCCSBlock 0 4]\n");
                header(&mut push, "INNER", 1);
                push(b"[% zCCSAtomicBlock 0 2]\n");
                message(&mut push, text, wav);
                push(b"[]\n[]\n");
            }
            Block::Empty { name } => {
                header(&mut push, name, 1);
                push(b"[% zCCSAtomicBlock 0 2]\n[% % 0 0]\n[]\n[]\n");
            }
            Block::Several { name } => {
                header(&mut push, name, 2);
                push(b"subBlock1=float:1\n[% zCCSAtomicBlock 0 2]\n[% % 0 0]\n[]\n[]\n");
            }
        }
        push(b"\t[]\n");
    }
    push(b"[]\n");
    out
}

/// Writes a binary archive: every object is its size, version, index, name and class, followed
/// by bare values.
#[derive(Default)]
struct Binary {
    out: Vec<u8>,
    open: Vec<usize>,
}

impl Binary {
    fn begin(&mut self, class: &str) -> &mut Self {
        self.open.push(self.out.len());
        self.out.extend([0; 10]);
        self.out.extend(b"%\0");
        self.string(class)
    }

    fn end(&mut self) -> &mut Self {
        let start = self.open.pop().unwrap();
        let size = (self.out.len() - start) as u32;
        self.out[start..start + 4].copy_from_slice(&size.to_le_bytes());
        self
    }

    fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.out.extend(bytes);
        self
    }

    fn string(&mut self, s: &str) -> &mut Self {
        self.bytes(s.as_bytes()).bytes(&[0])
    }

    fn header(&mut self, name: &str, count: i32) -> &mut Self {
        self.string(name).bytes(&count.to_le_bytes()).bytes(&0f32.to_le_bytes())
    }

    fn message(&mut self, text: &[u8], wav: &str) -> &mut Self {
        self.begin("oCMsgConversation:oCNpcMessage:zCEventMessage");
        self.bytes(&[0]).bytes(text).bytes(&[0]).string(wav).end()
    }
}

fn binary() -> Vec<u8> {
    let mut b = Binary::default();
    b.begin("zCCSLib").bytes(&(BLOCKS.len() as i32).to_le_bytes());
    for block in BLOCKS {
        b.begin("zCCSBlock");
        match *block {
            Block::Line { name, text, wav } => {
                b.header(name, 1).begin("zCCSAtomicBlock").message(text, wav).end();
            }
            Block::Nested { name, text, wav } => {
                b.header(name, 1).begin("zCCSBlock").header("INNER", 1);
                b.begin("zCCSAtomicBlock").message(text, wav).end().end();
            }
            Block::Empty { name } => {
                b.header(name, 1).begin("zCCSAtomicBlock").begin("%").end().end();
            }
            Block::Several { name } => {
                b.header(name, 2).bytes(&1f32.to_le_bytes());
                b.begin("zCCSAtomicBlock").begin("%").end().end();
            }
        }
        b.end();
    }
    b.end();

    let mut out =
        b"ZenGin Archive\nver 1\nzCArchiverGeneric\nBINARY\nsaveGame 0\nEND\nobjects 9    \nEND\n"
            .to_vec();
    out.extend(b.out);
    out
}

#[test]
fn both_encodings_read_the_same() {
    for data in [ascii(false), binary()] {
        let library = CutsceneLibrary::from_bytes(&data).unwrap();
        let names: Vec<_> = library.blocks.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(
            names,
            ["dia_a_01", "DIA_B_01", "DIA_E_01"],
            "sorted, unreadable blocks left out"
        );

        let a = library.block("DIA_A_01").unwrap();
        assert_eq!(
            a.message,
            ConversationMessage {
                sub_type: 0,
                text: "Hallo.".into(),
                name: "DIA_A_01.WAV".into(),
                state: None
            }
        );
        assert_eq!(library.block("dia_b_01").unwrap().message.text, "Grüß dich.");
        assert_eq!(library.block("DIA_E_01").unwrap().message.text, "");
        assert!(library.block("DIA_C_01").is_none());
        assert!(library.block("DIA_Z_99").is_none());
    }
}

#[test]
fn save_games_keep_message_state() {
    let library = CutsceneLibrary::from_bytes(&ascii(true)).unwrap();
    let state = library.block("DIA_B_01").unwrap().message.state;
    assert_eq!(state, Some(MessageState { high_priority: true, in_use: false, deleted: true }));
}

#[test]
fn broken_libraries_fail_cleanly() {
    for data in [ascii(false), binary()] {
        for len in 0..data.len() {
            let _ = CutsceneLibrary::from_bytes(&data[..len]);
        }
    }
    assert!(CutsceneLibrary::from_bytes(b"ZenGin Archive\n").is_err());
}

/// Gothic 1's German `OU.BIN`, if `ZENKIT_SAMPLES` points at a ZenKit checkout's
/// `tests/samples` with a copy of it named `ou.proprietary.bin`, as ZenKit's own tests want it.
/// The checks are ZenKit's.
#[test]
fn zenkit_gothic1_library() {
    let path =
        std::env::var_os("ZENKIT_SAMPLES").map(|d| PathBuf::from(d).join("ou.proprietary.bin"));
    let Some(data) = path.and_then(|path| std::fs::read(path).ok()) else {
        eprintln!("no ou.proprietary.bin in ZENKIT_SAMPLES, skipping");
        return;
    };
    let library = CutsceneLibrary::from_bytes(&data).unwrap();
    assert_eq!(library.blocks.len(), 7360);

    let message = &library.block("DIA_ARTO_PERM_15_00").unwrap().message;
    assert_eq!((message.sub_type, message.text.as_str()), (0, "Du redest nicht viel, was?"));
    assert_eq!(message.name, "DIA_ARTO_PERM_15_00.WAV");

    let message = &library.block("DIA_BaalKagan_WasDrin_13_01").unwrap().message;
    assert_eq!(message.text, "Ich kann dich auf viele Arten entlohnen.");
    assert_eq!(message.name, "DIA_BAALKAGAN_WASDRIN_13_01.WAV");

    let message = &library.block("DIA_BAALTARAN_INTOCASTLE_EXACTLY_15_00").unwrap().message;
    assert_eq!(message.text, "Stimmt genau.");
    assert!(library.block("nonexistent").is_none());
}

#[test]
fn libraries_round_trip_in_every_encoding() {
    use zenkit_rs_archive::Format;

    let library = CutsceneLibrary::from_bytes(&ascii(false)).unwrap();
    for format in [Format::Ascii, Format::Binary, Format::Binsafe] {
        let back = CutsceneLibrary::from_bytes(&library.to_bytes(format).unwrap()).unwrap();
        assert_eq!(back, library, "{format:?}");
    }
}

#[test]
fn csl_looks_like_the_games() {
    use zenkit_rs_archive::Format;

    let library = CutsceneLibrary {
        blocks: vec![zenkit_rs_cutscene::CutsceneBlock {
            name: "DIA_ARTO_PERM_15_00".into(),
            message: ConversationMessage {
                sub_type: 0,
                text: "Du redest nicht viel, was?".into(),
                name: "DIA_ARTO_PERM_15_00.WAV".into(),
                state: None,
            },
        }],
    };
    let expected = "ZenGin Archive\nver 1\nzCArchiverGeneric\nASCII\nsaveGame 0\nEND\n\
                    objects 4        \nEND\n\n\
                    [% zCCSLib 0 0]\n\
                    \tNumOfItems=int:1\n\
                    \t[% zCCSBlock 0 1]\n\
                    \t\tblockName=string:DIA_ARTO_PERM_15_00\n\
                    \t\tnumOfBlocks=int:1\n\
                    \t\tsubBlock0=float:0\n\
                    \t\t[% zCCSAtomicBlock 0 2]\n\
                    \t\t\t[% oCMsgConversation:oCNpcMessage:zCEventMessage 0 3]\n\
                    \t\t\t\tsubType=enum:0\n\
                    \t\t\t\ttext=string:Du redest nicht viel, was?\n\
                    \t\t\t\tname=string:DIA_ARTO_PERM_15_00.WAV\n\
                    \t\t\t[]\n\
                    \t\t[]\n\
                    \t[]\n\
                    []\n";
    let written = String::from_utf8(library.to_bytes(Format::Ascii).unwrap()).unwrap();
    assert_eq!(written, expected);
}

/// Rewriting the whole Gothic 1 library, if it is there, changes none of its lines.
#[test]
fn zenkit_gothic1_library_round_trips() {
    use zenkit_rs_archive::Format;

    let path =
        std::env::var_os("ZENKIT_SAMPLES").map(|d| PathBuf::from(d).join("ou.proprietary.bin"));
    let Some(data) = path.and_then(|path| std::fs::read(path).ok()) else {
        return;
    };
    let library = CutsceneLibrary::from_bytes(&data).unwrap();
    let back = CutsceneLibrary::from_bytes(&library.to_bytes(Format::Binsafe).unwrap()).unwrap();
    assert_eq!(back, library);
}

#[test]
fn save_games_write_message_state() {
    use zenkit_rs_archive::ArchiveWriter;
    use zenkit_rs_archive::Format;
    use zenkit_rs_archive::Header;

    let library = CutsceneLibrary::from_bytes(&ascii(true)).unwrap();
    let mut header = Header::new(Format::Binsafe);
    header.save_game = true;
    let mut w = ArchiveWriter::with_header(header);
    w.begin_object("%", "zCCSLib", 0);
    library.write_fields(&mut w);
    w.end_object();

    let back = CutsceneLibrary::from_bytes(&w.finish().unwrap()).unwrap();
    assert_eq!(back, library);
    assert!(back.blocks.iter().all(|b| b.message.state.is_some()));
}
