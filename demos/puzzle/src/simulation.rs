//! Discrete, renderer-independent rules and reversible history.

use alloc::collections::{BTreeMap, BTreeSet};

use bevy::prelude::{Message, Resource};

use crate::level::{Cell, Level};

/// One cardinal grid step. Row zero is north (up).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Decrease the row.
    Up,
    /// Increase the row.
    Down,
    /// Decrease the column.
    Left,
    /// Increase the column.
    Right,
}

impl Direction {
    fn destination(self, from: Cell) -> Cell {
        let (x, y) = match self {
            Self::Up => (0, -1),
            Self::Down => (0, 1),
            Self::Left => (-1, 0),
            Self::Right => (1, 0),
        };
        Cell::new(from.x + x, from.y + y)
    }
}

/// The only gameplay commands, shared by input, scripts, and future solvers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Walk one cell, pushing at most one block.
    Move(Direction),
    /// Restore the previous successful mutation, including restart.
    Undo,
    /// Reapply an undone mutation without re-running its rules.
    Redo,
    /// Restore the authored starting positions (undoable).
    Restart,
    /// Advance a solved level; history is scoped to the current level.
    NextLevel,
}

/// An immutable-by-consumers simulation snapshot, with stable block identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoardState {
    player: Cell,
    blocks: BTreeMap<String, Cell>,
    moves: u32,
    complete: bool,
}

impl BoardState {
    fn initial(level: &Level) -> Self {
        Self {
            player: level.player().position,
            blocks: level
                .blocks()
                .iter()
                .map(|o| (o.id.clone(), o.position))
                .collect(),
            moves: 0,
            complete: false,
        }
    }

    /// Current player cell; its authored ID is available on the level.
    pub fn player(&self) -> Cell {
        self.player
    }
    /// Block positions in stable ID order, independent of ECS allocation.
    pub fn blocks(&self) -> &BTreeMap<String, Cell> {
        &self.blocks
    }
    /// Successful walks/pushes since the starting state, restored by history.
    pub fn moves(&self) -> u32 {
        self.moves
    }
    /// Whether every target is occupied by a block.
    pub fn complete(&self) -> bool {
        self.complete
    }
    /// Stable ID of the block at a cell, if any.
    pub fn block_at(&self, cell: Cell) -> Option<&str> {
        self.blocks
            .iter()
            .find(|(_, p)| **p == cell)
            .map(|(id, _)| id.as_str())
    }
}

/// Why an attempted walk/push did not change state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockReason {
    /// The player or pushed block would hit a wall or leave the grid.
    Wall,
    /// A push would move more than one block.
    Block,
    /// Movement is disabled on a completed level until undo/restart/advance.
    Complete,
}

/// Why a history or progression action was a no-op.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IgnoreReason {
    /// No earlier mutation remains.
    NoUndo,
    /// No undone mutation remains.
    NoRedo,
    /// Already at the authored starting state.
    AtStart,
    /// Cannot advance an unsolved level.
    NotComplete,
    /// The last starter level has been completed.
    EndOfCampaign,
}

/// An ordered gameplay fact. Presentation must not infer these from keyboard input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// Player movement, emitted after any block movement.
    Moved {
        /// Authored player ID.
        id: String,
        /// Previous cell.
        from: Cell,
        /// New cell.
        to: Cell,
    },
    /// A single block moved.
    Pushed {
        /// Authored block ID.
        id: String,
        /// Previous cell.
        from: Cell,
        /// New cell.
        to: Cell,
    },
    /// Walk or push rejected with no history entry.
    Blocked {
        /// Attempted direction.
        direction: Direction,
        /// Obstruction category.
        reason: BlockReason,
    },
    /// Target occupancy changed, also emitted for history restoration.
    TargetChanged {
        /// Authored target ID.
        id: String,
        /// New occupying block ID, or none if uncovered.
        block: Option<String>,
    },
    /// Every target became covered.
    LevelComplete,
    /// History restoration changed a solved level back to unsolved.
    LevelReopened,
    /// History/restart restoration with the exact state at this action.
    Restored {
        /// Undo, redo, or restart command that caused restoration.
        action: Action,
        /// Resulting snapshot, even if later ticks have already changed the game.
        state: BoardState,
    },
    /// A level boundary with enough data to initialize event-only consumers.
    LevelStarted {
        /// New immutable level definition (boxed to keep the enum compact).
        level: Box<Level>,
        /// Initial board snapshot, before any later actions.
        state: BoardState,
    },
    /// An action made no change.
    Ignored(IgnoreReason),
}

/// Independent readers receive the same ordered stream through Bevy messages.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct GameplayEvent {
    /// Monotonic action number, including blocked/no-op actions (first is 1).
    pub sequence: u64,
    /// Authored level ID. Object IDs are scoped by this value.
    pub level_id: String,
    /// Move count after this action.
    pub moves: u32,
    /// Event payload; multiple facts may share an action number.
    pub kind: EventKind,
}

/// Deterministic campaign simulation. No clock, RNG, entities, or presentation state.
#[derive(Resource, Clone, Debug)]
pub struct Game {
    levels: Vec<Level>,
    index: usize,
    state: BoardState,
    undo: Vec<BoardState>,
    redo: Vec<BoardState>,
    sequence: u64,
}

impl Game {
    /// Create a campaign. Level IDs must be unique and the campaign nonempty.
    pub fn new(levels: Vec<Level>) -> Result<Self, &'static str> {
        if levels.is_empty() {
            return Err("a campaign needs at least one level");
        }
        let mut ids = BTreeSet::new();
        if levels.iter().any(|level| !ids.insert(level.id())) {
            return Err("campaign level IDs must be unique");
        }
        Ok(Self {
            state: BoardState::initial(&levels[0]),
            levels,
            index: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            sequence: 0,
        })
    }

    /// Current immutable level definition.
    pub fn level(&self) -> &Level {
        &self.levels[self.index]
    }
    /// Current immutable board snapshot.
    pub fn state(&self) -> &BoardState {
        &self.state
    }
    /// Zero-based campaign index.
    pub fn level_index(&self) -> usize {
        self.index
    }
    /// Number of levels in this campaign.
    pub fn level_count(&self) -> usize {
        self.levels.len()
    }
    /// Number of actions processed, including no-ops.
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    /// Whether undo is currently available.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    /// Whether redo is currently available.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Process exactly one command, returning facts in deterministic order.
    /// Only successful mutations consume history. A new mutation clears redo.
    pub fn step(&mut self, action: Action) -> Vec<GameplayEvent> {
        self.sequence += 1;
        let before = self.state.clone();
        let mut kinds = Vec::new();
        match action {
            Action::Move(direction) => self.move_player(direction, &mut kinds),
            Action::Undo => {
                if let Some(state) = self.undo.pop() {
                    self.redo.push(core::mem::replace(&mut self.state, state));
                    kinds.push(EventKind::Restored {
                        action,
                        state: self.state.clone(),
                    });
                } else {
                    kinds.push(EventKind::Ignored(IgnoreReason::NoUndo));
                }
            }
            Action::Redo => {
                if let Some(state) = self.redo.pop() {
                    self.undo.push(core::mem::replace(&mut self.state, state));
                    kinds.push(EventKind::Restored {
                        action,
                        state: self.state.clone(),
                    });
                } else {
                    kinds.push(EventKind::Ignored(IgnoreReason::NoRedo));
                }
            }
            Action::Restart => {
                let start = BoardState::initial(self.level());
                if start == self.state {
                    kinds.push(EventKind::Ignored(IgnoreReason::AtStart));
                } else {
                    self.undo.push(core::mem::replace(&mut self.state, start));
                    self.redo.clear();
                    kinds.push(EventKind::Restored {
                        action,
                        state: self.state.clone(),
                    });
                }
            }
            Action::NextLevel => {
                if !self.state.complete {
                    kinds.push(EventKind::Ignored(IgnoreReason::NotComplete));
                } else if self.index + 1 == self.levels.len() {
                    kinds.push(EventKind::Ignored(IgnoreReason::EndOfCampaign));
                } else {
                    self.index += 1;
                    self.state = BoardState::initial(self.level());
                    self.undo.clear();
                    self.redo.clear();
                    kinds.push(EventKind::LevelStarted {
                        level: Box::new(self.level().clone()),
                        state: self.state.clone(),
                    });
                }
            }
        }
        // A level boundary has its own reset event, not cross-level target diffs.
        if !kinds
            .iter()
            .any(|kind| matches!(kind, EventKind::LevelStarted { .. }))
        {
            for target in self.level().targets() {
                let old = before.block_at(target.position);
                let new = self.state.block_at(target.position);
                if old != new {
                    kinds.push(EventKind::TargetChanged {
                        id: target.id.clone(),
                        block: new.map(str::to_owned),
                    });
                }
            }
            match (before.complete, self.state.complete) {
                (false, true) => kinds.push(EventKind::LevelComplete),
                (true, false) => kinds.push(EventKind::LevelReopened),
                _ => {}
            }
        }
        kinds
            .into_iter()
            .map(|kind| GameplayEvent {
                sequence: self.sequence,
                level_id: self.level().id().to_owned(),
                moves: self.state.moves,
                kind,
            })
            .collect()
    }

    fn move_player(&mut self, direction: Direction, events: &mut Vec<EventKind>) {
        let from = self.state.player;
        let to = direction.destination(from);
        let block = self.state.block_at(to).map(str::to_owned);
        let pushed_to = direction.destination(to);
        let reason = if self.state.complete {
            Some(BlockReason::Complete)
        } else if self.level().is_wall(to) || (block.is_some() && self.level().is_wall(pushed_to)) {
            Some(BlockReason::Wall)
        } else if block.is_some() && self.state.block_at(pushed_to).is_some() {
            Some(BlockReason::Block)
        } else {
            None
        };
        if let Some(reason) = reason {
            events.push(EventKind::Blocked { direction, reason });
            return;
        }
        self.undo.push(self.state.clone());
        self.redo.clear();
        if let Some(id) = block {
            self.state.blocks.insert(id.clone(), pushed_to);
            events.push(EventKind::Pushed {
                id,
                from: to,
                to: pushed_to,
            });
        }
        self.state.player = to;
        self.state.moves += 1;
        self.state.complete = self
            .level()
            .targets()
            .iter()
            .all(|target| self.state.block_at(target.position).is_some());
        events.push(EventKind::Moved {
            id: self.level().player().id.clone(),
            from,
            to,
        });
    }
}
