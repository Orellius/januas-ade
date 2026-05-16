//! Januas terminal — wraps `alacritty_terminal::Term` for cell-grid storage.
//!
//! Purpose: own the screen model. PTY bytes go in via [`Terminal::feed`];
//! the renderer reads back the grid via [`Terminal::with_term`].
//! Why this file (vs inlining): the Warp research mandates per-block grid
//! isolation; centralizing the Term wrapper here makes spawning N grids
//! (one per block) trivial in a future slice.
//! Not responsibilities: PTY IO (pty crate), rendering (renderer crate).
//! Test strategy: feed known escape sequences, assert grid contents.

use std::sync::Arc;

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Config;
use alacritty_terminal::vte::ansi::Processor;

/// Passive event listener — we ignore Term events for v0.1 of S3.
#[derive(Clone, Default)]
pub struct NullEventListener;

impl EventListener for NullEventListener {
    fn send_event(&self, _event: Event) {}
}

/// A pair of `(cols, rows)` implementing `alacritty_terminal::grid::Dimensions`.
/// Local replacement for the test-only `term::test::TermSize`.
#[derive(Debug, Clone, Copy)]
pub struct GridSize {
    /// Number of columns in the visible grid.
    pub columns: usize,
    /// Number of rows currently visible (excludes scrollback).
    pub screen_lines: usize,
}

impl Dimensions for GridSize {
    fn columns(&self) -> usize {
        self.columns
    }

    fn screen_lines(&self) -> usize {
        self.screen_lines
    }

    fn total_lines(&self) -> usize {
        self.screen_lines
    }
}

/// One cell-grid terminal. Wraps `alacritty_terminal::Term` plus an ANSI processor.
pub struct Terminal {
    term: Arc<FairMutex<Term<NullEventListener>>>,
    parser: Processor,
    size: GridSize,
}

impl Terminal {
    /// Create a fresh empty terminal at the given cell dimensions.
    #[must_use]
    pub fn new(cols: u16, rows: u16) -> Self {
        let config = Config::default();
        let size = GridSize {
            columns: usize::from(cols),
            screen_lines: usize::from(rows),
        };
        let term = Term::new(config, &size, NullEventListener);
        Self {
            term: Arc::new(FairMutex::new(term)),
            parser: Processor::new(),
            size,
        }
    }

    /// Feed shell bytes through the ANSI parser into the grid.
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut term = self.term.lock();
        for byte in bytes {
            self.parser.advance(&mut *term, &[*byte]);
        }
    }

    /// Returns the current grid dimensions `(columns, rows)`.
    #[must_use]
    pub const fn dimensions(&self) -> (usize, usize) {
        (self.size.columns, self.size.screen_lines)
    }

    /// Acquire a read-only lock on the underlying `Term` for grid inspection.
    pub fn with_term<R, F: FnOnce(&Term<NullEventListener>) -> R>(&self, f: F) -> R {
        let term = self.term.lock();
        f(&term)
    }
}
