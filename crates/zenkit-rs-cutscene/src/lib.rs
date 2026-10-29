//! Cutscene libraries, also called message databases: every line of dialogue in the game, with
//! the sound file of its recording.
//!
//! Scripts say a line with `AI_Output(self, other, "DIA_ARTO_PERM_15_00")`, and the engine looks
//! that name up here. The library ships as a binary archive (`OU.BIN`) or an ASCII one
//! (`OU.CSL`), in `_work/Data/Scripts/Content/Cutscene`.
//!
//! ```no_run
//! # fn main() -> zenkit_rs_core::Result<()> {
//! use zenkit_rs_cutscene::CutsceneLibrary;
//!
//! let library = CutsceneLibrary::from_bytes(&std::fs::read("OU.BIN").unwrap())?;
//! if let Some(line) = library.block("DIA_ARTO_PERM_15_00") {
//!     println!("{}, spoken in {}", line.message.text, line.message.name);
//! }
//! # Ok(()) }
//! ```

use std::cmp::Ordering;

use tracing::warn;
use zenkit_rs_archive::Archive;
use zenkit_rs_archive::ObjectHeader;
use zenkit_rs_core::Asset;
use zenkit_rs_core::Error;
use zenkit_rs_core::Result;
use zenkit_rs_core::ResultExt;
use zenkit_rs_core::text;

const FORMAT: &str = "cutscene library";

/// The deepest blocks may nest in each other before the data is taken to be broken.
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CutsceneLibrary {
    /// Sorted by name, ignoring case, for [`block`](Self::block).
    pub blocks: Vec<CutsceneBlock>,
}

impl CutsceneLibrary {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut archive = Archive::new(data)?;
        let object = archive.expect_object(b"zCCSLib")?;
        let library = Self::read_fields(&mut archive)?;
        end(&mut archive, &object)?;
        Ok(library)
    }

    /// Reads the contents of a `zCCSLib` object whose header has already been read.
    pub fn read_fields(archive: &mut Archive) -> Result<Self> {
        let count = archive.int()?.max(0) as usize;
        let mut blocks = Vec::with_capacity(count.min(1 << 16));
        for i in 0..count {
            let block =
                read_block_object(archive, b"zCCSBlock").context(|| format!("block {i}"))?;
            blocks.extend(block);
        }
        blocks.sort_by(|a, b| compare(&a.name, &b.name));
        Ok(Self { blocks })
    }

    /// Finds a block by name, ignoring case as the engine does.
    pub fn block(&self, name: &str) -> Option<&CutsceneBlock> {
        let index = self.blocks.binary_search_by(|block| compare(&block.name, name)).ok()?;
        Some(&self.blocks[index])
    }
}

impl Asset for CutsceneLibrary {
    fn from_bytes(data: &[u8]) -> Result<Self> {
        CutsceneLibrary::from_bytes(data)
    }
}

/// Compares names ignoring ASCII case. The sort is stable, so names that only differ in case
/// keep their order from the file.
fn compare(a: &str, b: &str) -> Ordering {
    a.chars().map(|c| c.to_ascii_uppercase()).cmp(b.chars().map(|c| c.to_ascii_uppercase()))
}

/// One line of dialogue, under the name scripts use for it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CutsceneBlock {
    pub name: String,
    pub message: ConversationMessage,
}

impl CutsceneBlock {
    /// Reads the fields of a `zCCSBlock` object, or of a class built on it, whose header has
    /// already been read.
    ///
    /// The format allows a block to hold several sub-blocks, but the games only ever store one,
    /// and how several would be laid out is unknown. A block with another number logs a warning
    /// and gives `None`, leaving the rest of the object unread.
    pub fn read_fields(archive: &mut Archive) -> Result<Option<Self>> {
        let name = archive.name()?;
        let count = archive.int()?;
        if count != 1 {
            warn!("skipping block {name:?}, which holds {count} sub-blocks rather than one");
            return Ok(None);
        }
        // The time the sub-block starts at, which is always 0.
        archive.float()?;

        let mut message = None;
        // A block may hold a further block rather than the message itself; only the message at
        // the bottom matters. Read with a stack, so that nesting cannot overflow the call stack.
        let mut open = Vec::new();
        loop {
            let offset = archive.offset();
            let Some(object) = archive.begin_object()? else {
                return Err(Error::malformed(
                    FORMAT,
                    format!("expected a block at offset {offset}"),
                ));
            };
            match object.class {
                b"zCCSAtomicBlock" => {
                    message =
                        ConversationMessage::read(archive).context(|| format!("block {name:?}"))?;
                    end(archive, &object)?;
                    break;
                }
                b"zCCSBlock" if open.len() < MAX_DEPTH => {
                    archive.name()?;
                    if archive.int()? != 1 {
                        warn!("block {name:?} nests a block with other than one sub-block");
                        archive.skip_object()?;
                        break;
                    }
                    archive.float()?;
                    open.push(object);
                }
                _ if object.is_null() => {
                    end(archive, &object)?;
                    break;
                }
                other => {
                    warn!(
                        "skipping sub-block of class {:?} in block {name:?}",
                        text::latin1(other)
                    );
                    archive.skip_object()?;
                    break;
                }
            }
        }
        while let Some(object) = open.pop() {
            end(archive, &object)?;
        }

        Ok(message.map(|message| Self { name, message }))
    }
}

/// What is said, and the recording of it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConversationMessage {
    pub sub_type: u32,
    /// The line as subtitled, in the language the game was localised to. It is decoded as
    /// Latin-1, which keeps every byte, so text in another code page can be recovered by turning
    /// each `char` back into a `u8`.
    pub text: String,
    /// The recording, like `DIA_ARTO_PERM_15_00.WAV`.
    pub name: String,
    /// Save games only.
    pub state: Option<MessageState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MessageState {
    pub high_priority: bool,
    pub in_use: bool,
    pub deleted: bool,
}

impl ConversationMessage {
    /// Reads the message object inside an atomic block. Gives `None` for a null object or one
    /// of another class, which is skipped.
    fn read(archive: &mut Archive) -> Result<Option<Self>> {
        let offset = archive.offset();
        let Some(object) = archive.begin_object()? else {
            return Err(Error::malformed(FORMAT, format!("expected a message at offset {offset}")));
        };
        if object.class != b"oCMsgConversation:oCNpcMessage:zCEventMessage" {
            if !object.is_null() {
                warn!("skipping message of class {:?}", text::latin1(object.class));
            }
            archive.skip_object()?;
            return Ok(None);
        }

        let sub_type = archive.enumeration()?;
        let state = if archive.header().save_game {
            Some(MessageState {
                high_priority: archive.bool()?,
                in_use: archive.bool()?,
                deleted: archive.bool()?,
            })
        } else {
            None
        };
        let message = Self { sub_type, text: archive.name()?, name: archive.name()?, state };
        end(archive, &object)?;
        Ok(Some(message))
    }
}

/// Reads a whole block object of `class`. Null objects and blocks that cannot be read are
/// skipped.
fn read_block_object(archive: &mut Archive, class: &[u8]) -> Result<Option<CutsceneBlock>> {
    let offset = archive.offset();
    let Some(object) = archive.begin_object()? else {
        return Err(Error::malformed(FORMAT, format!("expected a block at offset {offset}")));
    };
    if object.class != class {
        if !object.is_null() {
            warn!("skipping block of class {:?}", text::latin1(object.class));
        }
        archive.skip_object()?;
        return Ok(None);
    }

    let Some(block) = CutsceneBlock::read_fields(archive)? else {
        archive.skip_object()?;
        return Ok(None);
    };
    end(archive, &object)?;
    Ok(Some(block))
}

/// Closes an object, skipping whatever was left unread in it.
fn end(archive: &mut Archive, object: &ObjectHeader) -> Result<()> {
    if !archive.end_object()? {
        warn!("object {:?} has unread fields", text::latin1(object.class));
        archive.skip_object()?;
    }
    Ok(())
}
