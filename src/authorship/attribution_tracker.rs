//! Attribution tracking through file changes
//!
//! This library maintains attribution ranges as files are edited, preserving
//! authorship information even through moves, edits, and whitespace changes.

use crate::authorship::move_detection::{DeletedLine, InsertedLine, detect_moves};
use crate::authorship::working_log::CheckpointKind;
use crate::error::GitAiError;
use diff_match_patch_rs::dmp::Diff;
use diff_match_patch_rs::Ops;
use similar::{Algorithm, DiffOp, capture_diff_slices};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

pub const INITIAL_ATTRIBUTION_TS: u128 = 42;

/// Represents a single attribution range in the file.
/// Ranges can overlap (multiple authors can be attributed to the same text).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Attribution {
    /// Character position where this attribution starts (inclusive)
    pub start: usize,
    /// Character position where this attribution ends (exclusive)
    pub end: usize,
    /// Identifier for the author of this range
    pub author_id: String,
    /// Timestamp of the attribution (in milliseconds since epoch)
    pub ts: u128,
}

/// Represents attribution for a range of lines.
/// Both start_line and end_line are inclusive (1-indexed).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct LineAttribution {
    /// Line number where this attribution starts (inclusive, 1-indexed)
    pub start_line: u32,
    /// Line number where this attribution ends (inclusive, 1-indexed)
    pub end_line: u32,
    /// Identifier for the author of this range
    pub author_id: String,
    /// Author ID that was overwritten by this attribution (e.g., if Alice wrote this line originally, then Bob edited it, overwrote=Alice because her edit was writen over)
    #[serde(default)]
    pub overrode: Option<String>,
}

impl LineAttribution {
    pub fn new(
        start_line: u32,
        end_line: u32,
        author_id: String,
        overrode: Option<String>,
    ) -> Self {
        LineAttribution {
            start_line,
            end_line,
            author_id,
            overrode,
        }
    }

    /// Returns the number of lines this attribution covers
    #[allow(dead_code)]
    pub fn line_count(&self) -> u32 {
        if self.start_line > self.end_line {
            0
        } else {
            self.end_line - self.start_line + 1
        }
    }

    /// Checks if this line attribution is empty
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.start_line > self.end_line
    }

    /// Checks if this attribution overlaps with a given line range (inclusive)
    #[allow(dead_code)]
    pub fn overlaps(&self, start_line: u32, end_line: u32) -> bool {
        self.start_line <= end_line && self.end_line >= start_line
    }

    /// Returns the overlapping portion of this attribution with a given line range
    #[allow(dead_code)]
    pub fn intersection(&self, start_line: u32, end_line: u32) -> Option<(u32, u32)> {
        let overlap_start = self.start_line.max(start_line);
        let overlap_end = self.end_line.min(end_line);

        if overlap_start <= overlap_end {
            Some((overlap_start, overlap_end))
        } else {
            None
        }
    }
}

impl Attribution {
    pub fn new(start: usize, end: usize, author_id: String, ts: u128) -> Self {
        Attribution {
            start,
            end,
            author_id,
            ts,
        }
    }

    /// Returns the length of this attribution range
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Checks if this attribution is empty
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Checks if this attribution overlaps with a given range
    pub fn overlaps(&self, start: usize, end: usize) -> bool {
        self.start < end && self.end > start
    }

    /// Returns the overlapping portion of this attribution with a given range
    pub fn intersection(&self, start: usize, end: usize) -> Option<(usize, usize)> {
        let overlap_start = self.start.max(start);
        let overlap_end = self.end.min(end);

        if overlap_start < overlap_end {
            Some((overlap_start, overlap_end))
        } else {
            None
        }
    }
}

/// Represents a deletion operation from the diff
#[derive(Debug, Clone)]
pub(crate) struct Deletion {
    /// Start position in old content
    pub(crate) start: usize,
    /// End position in old content
    pub(crate) end: usize,
    /// The deleted bytes (may not be valid UTF-8)
    #[allow(dead_code)]
    pub(crate) bytes: Vec<u8>,
}

/// Represents an insertion operation from the diff
#[derive(Debug, Clone)]
pub(crate) struct Insertion {
    /// Start position in new content
    pub(crate) start: usize,
    /// End position in new content
    pub(crate) end: usize,
    /// The inserted bytes (may not be valid UTF-8)
    #[allow(dead_code)]
    pub(crate) bytes: Vec<u8>,
}

/// Information about a detected move operation
#[derive(Debug, Clone)]
pub(crate) struct MoveMapping {
    /// The deletion that was moved
    pub(crate) deletion_idx: usize,
    /// The insertion where it was moved to
    pub(crate) insertion_idx: usize,
    /// Range within the deletion text that maps to the insertion (start, end) exclusive bounds
    pub(crate) source_range: (usize, usize),
    /// Range within the insertion text where the deletion text lands (start, end) exclusive bounds
    pub(crate) target_range: (usize, usize),
}

#[derive(Debug, Clone)]
struct LineMetadata {
    number: usize,
    start: usize,
    end: usize,
    text: String,
}

fn collect_line_metadata(content: &str) -> Vec<LineMetadata> {
    let mut metadata = Vec::new();
    let mut line_start = 0usize;
    let mut line_number = 1usize;

    for (idx, ch) in content.char_indices() {
        if ch == '\n' {
            let slice = &content[line_start..idx];
            let mut text = slice.to_string();
            if text.ends_with('\r') {
                text.pop();
            }
            metadata.push(LineMetadata {
                number: line_number,
                start: line_start,
                end: idx + 1,
                text,
            });
            line_start = idx + 1;
            line_number += 1;
        }
    }

    if line_start < content.len() {
        let slice = &content[line_start..content.len()];
        let mut text = slice.to_string();
        if text.ends_with('\r') {
            text.pop();
        }
        metadata.push(LineMetadata {
            number: line_number,
            start: line_start,
            end: content.len(),
            text,
        });
    }

    metadata
}

#[derive(Clone, Debug)]
struct Token {
    lexeme: String,
    start: usize,
    end: usize,
    line: usize,
}

impl PartialEq for Token {
    fn eq(&self, other: &Self) -> bool {
        self.lexeme == other.lexeme
    }
}

impl Eq for Token {}

impl Hash for Token {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.lexeme.hash(state);
    }
}

impl PartialOrd for Token {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Token {
    fn cmp(&self, other: &Self) -> Ordering {
        self.lexeme.cmp(&other.lexeme)
    }
}

#[derive(Default)]
struct DiffComputation {
    diffs: Vec<Diff<u8>>,
    substantive_new_ranges: Vec<(usize, usize)>,
}

/// Configuration for the attribution tracker
pub struct AttributionConfig {
    move_lines_threshold: usize,
}

impl Default for AttributionConfig {
    fn default() -> Self {
        AttributionConfig {
            move_lines_threshold: 3,
        }
    }
}

/// Main attribution tracker
pub struct AttributionTracker {
    config: AttributionConfig,
}

impl AttributionTracker {
    /// Create a new attribution tracker with default configuration
    pub fn new() -> Self {
        AttributionTracker {
            config: AttributionConfig::default(),
        }
    }

    /// Create a new attribution tracker with custom configuration
    #[allow(dead_code)]
    pub fn with_config(config: AttributionConfig) -> Self {
        AttributionTracker {
            config,
        }
    }

    fn compute_diffs(
        &self,
        old_content: &str,
        new_content: &str,
    ) -> Result<DiffComputation, GitAiError> {
        let old_lines = collect_line_metadata(old_content);
        let new_lines = collect_line_metadata(new_content);

        let old_line_slices: Vec<&str> = old_lines
            .iter()
            .map(|line| &old_content[line.start..line.end])
            .collect();
        let new_line_slices: Vec<&str> = new_lines
            .iter()
            .map(|line| &new_content[line.start..line.end])
            .collect();

        let line_ops =
            capture_diff_slices(Algorithm::Myers, &old_line_slices, &new_line_slices);

        let mut computation = DiffComputation::default();
        let mut pending_changed: Vec<DiffOp> = Vec::new();

        for op in line_ops.into_iter() {
            if matches!(op, DiffOp::Equal { .. }) {
                if !pending_changed.is_empty() {
                    self.process_changed_hunk(
                        &pending_changed,
                        &old_lines,
                        &new_lines,
                        old_content,
                        new_content,
                        &mut computation,
                    )?;
                    pending_changed.clear();
                }

                self.push_equal_lines(op, &old_lines, old_content, &mut computation.diffs)?;
            } else {
                pending_changed.push(op);
            }
        }

        if !pending_changed.is_empty() {
            self.process_changed_hunk(
                &pending_changed,
                &old_lines,
                &new_lines,
                old_content,
                new_content,
                &mut computation,
            )?;
        }

        computation.substantive_new_ranges = merge_ranges(computation.substantive_new_ranges);

        Ok(computation)
    }

    fn push_equal_lines(
        &self,
        op: DiffOp,
        old_lines: &[LineMetadata],
        old_content: &str,
        diffs: &mut Vec<Diff<u8>>,
    ) -> Result<(), GitAiError> {
        if let DiffOp::Equal {
            old_index, len, ..
        } = op
        {
            if len == 0 {
                return Ok(());
            }

            let (start, end) = line_range_to_byte_range(
                old_lines,
                old_index,
                old_index + len,
                old_content.len(),
            );

            if start < end {
                diffs.push(Diff::<u8>::new(
                    Ops::Equal,
                    old_content[start..end].as_bytes(),
                ));
            }

            return Ok(());
        }

        Err(GitAiError::Generic(
            "Expected equal operation in push_equal_lines".to_string(),
        ))
    }

    fn process_changed_hunk(
        &self,
        ops: &[DiffOp],
        old_lines: &[LineMetadata],
        new_lines: &[LineMetadata],
        old_content: &str,
        new_content: &str,
        computation: &mut DiffComputation,
    ) -> Result<(), GitAiError> {
        if ops.is_empty() {
            return Ok(());
        }

        let (old_start_line, old_end_line) = hunk_line_bounds(ops, true);
        let (new_start_line, new_end_line) = hunk_line_bounds(ops, false);

        let (old_start, old_end) =
            line_range_to_byte_range(old_lines, old_start_line, old_end_line, old_content.len());
        let (new_start, new_end) =
            line_range_to_byte_range(new_lines, new_start_line, new_end_line, new_content.len());

        let (mut hunk_diffs, substantive_ranges) = build_token_aligned_diffs(
            old_content,
            new_content,
            (old_start, old_end),
            (new_start, new_end),
            old_start_line + 1,
            new_start_line + 1,
        );

        computation.diffs.append(&mut hunk_diffs);
        computation.substantive_new_ranges
            .extend(substantive_ranges.into_iter());

        Ok(())
    }

    /// Attribute all unattributed ranges to the given author
    pub fn attribute_unattributed_ranges(
        &self,
        content: &str,
        prev_attributions: &[Attribution],
        author: &str,
        ts: u128,
    ) -> Vec<Attribution> {
        let mut attributions = prev_attributions.to_vec();
        let mut unattributed_char_idxs = Vec::new();

        // Find all unattributed character positions
        for i in 0..content.len() {
            if !attributions.iter().any(|a| a.overlaps(i, i + 1)) {
                unattributed_char_idxs.push(i);
            }
        }

        // Sort the unattributed character indices by position
        unattributed_char_idxs.sort();

        // Group contiguous unattributed ranges
        let mut contiguous_ranges = Vec::new();
        if !unattributed_char_idxs.is_empty() {
            let mut start = unattributed_char_idxs[0];
            let mut end = start + 1;

            for i in 1..unattributed_char_idxs.len() {
                let current = unattributed_char_idxs[i];
                if current == end {
                    // Contiguous with previous range
                    end = current + 1;
                } else {
                    // Gap found, save current range and start new one
                    contiguous_ranges.push((start, end));
                    start = current;
                    end = current + 1;
                }
            }
            // Don't forget the last range
            contiguous_ranges.push((start, end));
        }

        // Create attributions for each contiguous unattributed range
        for (start, end) in contiguous_ranges {
            attributions.push(Attribution::new(start, end, author.to_string(), ts));
        }

        attributions
    }

    /// Update attributions from old content to new content
    ///
    /// # Arguments
    /// * `old_content` - The previous version of the file
    /// * `new_content` - The new version of the file
    /// * `old_attributions` - Attributions from the previous version
    /// * `current_author` - Author ID to use for new changes
    ///
    /// # Returns
    /// A vector of updated attributions for the new content
    pub fn update_attributions(
        &self,
        old_content: &str,
        new_content: &str,
        old_attributions: &[Attribution],
        current_author: &str,
        ts: u128,
    ) -> Result<Vec<Attribution>, GitAiError> {
        // Phase 1: Compute diff
        let diff_result = self.compute_diffs(old_content, new_content)?;

        // Phase 2: Build deletion and insertion catalogs
        let (deletions, insertions) = self.build_diff_catalog(&diff_result.diffs);

        // Phase 3: Detect move operations
        let move_mappings = self.detect_moves(old_content, new_content, &deletions, &insertions);

        // Phase 4: Transform attributions through the diff
        let new_attributions = self.transform_attributions(
            &diff_result.diffs,
            old_attributions,
            current_author,
            &insertions,
            &move_mappings,
            ts,
            &diff_result.substantive_new_ranges,
        );

        // Phase 5: Merge and clean up
        Ok(self.merge_attributions(new_attributions))
    }

    /// Build catalogs of deletions and insertions from the diff
    fn build_diff_catalog(&self, diffs: &[Diff<u8>]) -> (Vec<Deletion>, Vec<Insertion>) {
        let mut deletions = Vec::new();
        let mut insertions = Vec::new();

        let mut old_pos = 0;
        let mut new_pos = 0;

        for diff in diffs {
            let op = diff.op();
            match op {
                Ops::Equal => {
                    let len = diff.data().len();
                    old_pos += len;
                    new_pos += len;
                }
                Ops::Delete => {
                    let bytes = diff.data();
                    let len = bytes.len();
                    deletions.push(Deletion {
                        start: old_pos,
                        end: old_pos + len,
                        bytes: bytes.to_vec(),
                    });
                    old_pos += len;
                }
                Ops::Insert => {
                    let bytes = diff.data();
                    let len = bytes.len();
                    insertions.push(Insertion {
                        start: new_pos,
                        end: new_pos + len,
                        bytes: bytes.to_vec(),
                    });
                    new_pos += len;
                }
            }
        }

        (deletions, insertions)
    }

    /// Detect move operations between deletions and insertions
    fn detect_moves(
        &self,
        old_content: &str,
        new_content: &str,
        deletions: &[Deletion],
        insertions: &[Insertion],
    ) -> Vec<MoveMapping> {
        let threshold = self.config.move_lines_threshold;
        if threshold == 0 || deletions.is_empty() || insertions.is_empty() {
            return Vec::new();
        }

        let old_lines = collect_line_metadata(old_content);
        let new_lines = collect_line_metadata(new_content);

        let old_line_map: HashMap<usize, LineMetadata> = old_lines
            .iter()
            .cloned()
            .map(|line| (line.number, line))
            .collect();
        let new_line_map: HashMap<usize, LineMetadata> = new_lines
            .iter()
            .cloned()
            .map(|line| (line.number, line))
            .collect();

        let mut inserted_lines: Vec<InsertedLine> = Vec::new();
        for (insertion_idx, insertion) in insertions.iter().enumerate() {
            for line in new_lines.iter() {
                if line.start < insertion.end && line.end > insertion.start {
                    inserted_lines.push(InsertedLine::new(
                        line.text.clone(),
                        line.number,
                        insertion_idx,
                    ));
                }
            }
        }

        let mut deleted_lines: Vec<DeletedLine> = Vec::new();
        for (deletion_idx, deletion) in deletions.iter().enumerate() {
            for line in old_lines.iter() {
                if line.start < deletion.end && line.end > deletion.start {
                    deleted_lines.push(DeletedLine::new(
                        line.text.clone(),
                        line.number,
                        deletion_idx,
                    ));
                }
            }
        }

        if inserted_lines.is_empty() || deleted_lines.is_empty() {
            return Vec::new();
        }

        let mut inserted_lines_slice = inserted_lines;
        let mut deleted_lines_slice = deleted_lines;
        let line_mappings = detect_moves(
            inserted_lines_slice.as_mut_slice(),
            deleted_lines_slice.as_mut_slice(),
            threshold,
        );

        let mut move_mappings = Vec::new();

        'mapping: for line_mapping in line_mappings {
            if line_mapping.deleted.is_empty() || line_mapping.inserted.is_empty() {
                continue;
            }
            if line_mapping.deleted.len() != line_mapping.inserted.len() {
                continue;
            }

            let deletion_idx = line_mapping.deleted[0].deletion_idx;
            if !line_mapping
                .deleted
                .iter()
                .all(|line| line.deletion_idx == deletion_idx)
            {
                continue;
            }

            let insertion_idx = line_mapping.inserted[0].insertion_idx;
            if !line_mapping
                .inserted
                .iter()
                .all(|line| line.insertion_idx == insertion_idx)
            {
                continue;
            }

            let deletion = match deletions.get(deletion_idx) {
                Some(value) => value,
                None => continue,
            };
            let insertion = match insertions.get(insertion_idx) {
                Some(value) => value,
                None => continue,
            };

            let mut source_start_opt: Option<usize> = None;
            let mut source_end_opt: Option<usize> = None;
            for deleted_line in &line_mapping.deleted {
                let meta = match old_line_map.get(&deleted_line.line_number) {
                    Some(meta) => meta,
                    None => continue 'mapping,
                };
                let start = meta.start.max(deletion.start);
                let end = meta.end.min(deletion.end);
                if start >= end {
                    continue 'mapping;
                }
                let rel_start = start - deletion.start;
                let rel_end = end - deletion.start;
                if source_start_opt.is_none() {
                    source_start_opt = Some(rel_start);
                }
                source_end_opt = Some(rel_end);
            }

            let mut target_start_opt: Option<usize> = None;
            let mut target_end_opt: Option<usize> = None;
            for inserted_line in &line_mapping.inserted {
                let meta = match new_line_map.get(&inserted_line.line_number) {
                    Some(meta) => meta,
                    None => continue 'mapping,
                };
                let start = meta.start.max(insertion.start);
                let end = meta.end.min(insertion.end);
                if start >= end {
                    continue 'mapping;
                }
                let rel_start = start - insertion.start;
                let rel_end = end - insertion.start;
                if target_start_opt.is_none() {
                    target_start_opt = Some(rel_start);
                }
                target_end_opt = Some(rel_end);
            }

            let (source_start, source_end) = match (source_start_opt, source_end_opt) {
                (Some(start), Some(end)) if start < end => (start, end),
                _ => continue,
            };
            let (target_start, target_end) = match (target_start_opt, target_end_opt) {
                (Some(start), Some(end)) if start < end => (start, end),
                _ => continue,
            };

            move_mappings.push(MoveMapping {
                deletion_idx,
                insertion_idx,
                source_range: (source_start, source_end),
                target_range: (target_start, target_end),
            });
        }

        move_mappings
    }

    /// Transform attributions through the diff
    fn transform_attributions(
        &self,
        diffs: &[Diff<u8>],
        old_attributions: &[Attribution],
        current_author: &str,
        insertions: &[Insertion],
        move_mappings: &[MoveMapping],
        ts: u128,
        substantive_new_ranges: &[(usize, usize)],
    ) -> Vec<Attribution> {
        let mut new_attributions = Vec::new();

        // Build lookup maps for moves
        let mut deletion_to_move: HashMap<usize, Vec<&MoveMapping>> = HashMap::new();
        let mut insertion_move_ranges: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();

        for mapping in move_mappings {
            let entry = deletion_to_move.entry(mapping.deletion_idx).or_default();
            if !entry.iter().any(|existing| {
                existing.source_range == mapping.source_range
                    && existing.target_range == mapping.target_range
            }) {
                entry.push(mapping);
            }
            insertion_move_ranges
                .entry(mapping.insertion_idx)
                .or_default()
                .push(mapping.target_range);
        }

        for mappings in deletion_to_move.values_mut() {
            mappings.sort_by_key(|m| m.source_range.0);
        }

        let mut old_pos = 0;
        let mut new_pos = 0;
        let mut deletion_idx = 0;
        let mut insertion_idx = 0;

        for diff in diffs {
            let op = diff.op();
            let len = diff.data().len();

            match op {
                Ops::Equal => {
                    // Unchanged text: transform attributions directly
                    let old_range = (old_pos, old_pos + len);
                    let new_range = (new_pos, new_pos + len);

                    for attr in old_attributions {
                        if let Some((overlap_start, overlap_end)) =
                            attr.intersection(old_range.0, old_range.1)
                        {
                            // Transform to new position
                            let offset_in_range = overlap_start - old_range.0;
                            let overlap_len = overlap_end - overlap_start;

                            new_attributions.push(Attribution::new(
                                new_range.0 + offset_in_range,
                                new_range.0 + offset_in_range + overlap_len,
                                attr.author_id.clone(),
                                attr.ts.clone(),
                            ));
                        }
                    }

                    old_pos += len;
                    new_pos += len;
                }
                Ops::Delete => {
                    let deletion_range = (old_pos, old_pos + len);

                    // Check if this deletion is part of a move
                    if let Some(mappings) = deletion_to_move.get(&deletion_idx) {
                        for mapping in mappings {
                            let insertion = &insertions[mapping.insertion_idx];
                            let source_start = deletion_range.0 + mapping.source_range.0;
                            let source_end = deletion_range.0 + mapping.source_range.1;

                            if source_start < source_end {
                                let target_start = insertion.start + mapping.target_range.0;

                                for attr in old_attributions {
                                    if let Some((overlap_start, overlap_end)) =
                                        attr.intersection(source_start, source_end)
                                    {
                                        let offset_in_source = overlap_start - source_start;
                                        let new_start = target_start + offset_in_source;
                                        let new_end = new_start + (overlap_end - overlap_start);

                                        if new_start < new_end {
                                            new_attributions.push(Attribution::new(
                                                new_start,
                                                new_end,
                                                attr.author_id.clone(),
                                                attr.ts,
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // else: True deletion - attributions are lost

                    old_pos += len;
                    deletion_idx += 1;
                }
                Ops::Insert => {
                    // Check if this insertion is from a detected move
                    if let Some(ranges) = insertion_move_ranges.remove(&insertion_idx) {
                        let mut covered = ranges;
                        covered.sort_by_key(|r| r.0);

                        let mut merged: Vec<(usize, usize)> = Vec::new();
                        for (start, end) in covered {
                            if start >= end {
                                continue;
                            }

                            if let Some(last) = merged.last_mut() {
                                if start <= last.1 {
                                    last.1 = last.1.max(end);
                                } else {
                                    merged.push((start, end));
                                }
                            } else {
                                merged.push((start, end));
                            }
                        }

                        let mut cursor = 0usize;
                        for (start, end) in merged {
                            let clamped_start = start.min(len);
                            let clamped_end = end.min(len);

                            if cursor < clamped_start {
                                new_attributions.push(Attribution::new(
                                    new_pos + cursor,
                                    new_pos + clamped_start,
                                    current_author.to_string(),
                                    ts,
                                ));
                            }

                            cursor = cursor.max(clamped_end);
                        }

                        if cursor < len {
                            new_attributions.push(Attribution::new(
                                new_pos + cursor,
                                new_pos + len,
                                current_author.to_string(),
                                ts,
                            ));
                        }

                        new_pos += len;
                        insertion_idx += 1;
                        continue;
                    }

                    // Add attribution for this insertion
                    let insertion_range = (new_pos, new_pos + len);
                    let is_substantive_insert =
                        ranges_intersect(substantive_new_ranges, insertion_range);
                    let (author_id, attribution_ts) = if is_substantive_insert {
                        (current_author.to_string(), ts)
                    } else if let Some(attr) = new_attributions.last() {
                        (attr.author_id.clone(), attr.ts)
                    } else if let Some(attr) = find_attribution_for_insertion(old_attributions, old_pos) {
                        (attr.author_id.clone(), attr.ts)
                    } else {
                        (current_author.to_string(), ts)
                    };

                    new_attributions.push(Attribution::new(
                        new_pos,
                        new_pos + len,
                        author_id,
                        attribution_ts,
                    ));

                    new_pos += len;
                    insertion_idx += 1;
                }
            }
        }

        new_attributions
    }

    /// Merge and clean up attributions
    fn merge_attributions(&self, mut attributions: Vec<Attribution>) -> Vec<Attribution> {
        if attributions.is_empty() {
            return attributions;
        }

        // Sort by start position
        attributions.sort_by_key(|a| (a.start, a.end, a.author_id.clone()));

        // Remove exact duplicates
        attributions.dedup();

        attributions
    }
}

fn line_span_for_op(op: &DiffOp, for_old: bool) -> (usize, usize) {
    match (op, for_old) {
        (DiffOp::Equal { old_index, len, .. }, true) => (*old_index, *old_index + *len),
        (DiffOp::Equal { new_index, len, .. }, false) => (*new_index, *new_index + *len),
        (DiffOp::Delete {
            old_index, old_len, ..
        }, true) => (*old_index, *old_index + *old_len),
        (DiffOp::Delete { new_index, .. }, false) => (*new_index, *new_index),
        (DiffOp::Insert {
            old_index, .. }, true) => (*old_index, *old_index),
        (DiffOp::Insert {
            new_index, new_len, ..
        }, false) => (*new_index, *new_index + *new_len),
        (DiffOp::Replace {
            old_index, old_len, ..
        }, true) => (*old_index, *old_index + *old_len),
        (DiffOp::Replace {
            new_index, new_len, ..
        }, false) => (*new_index, *new_index + *new_len),
    }
}

fn hunk_line_bounds(ops: &[DiffOp], for_old: bool) -> (usize, usize) {
    let mut start = usize::MAX;
    let mut end = 0usize;

    for op in ops {
        let (s, e) = line_span_for_op(op, for_old);
        start = start.min(s);
        end = end.max(e);
    }

    if start == usize::MAX {
        (0, 0)
    } else {
        (start, end)
    }
}

fn line_range_to_byte_range(
    lines: &[LineMetadata],
    start_idx: usize,
    end_idx: usize,
    content_len: usize,
) -> (usize, usize) {
    if start_idx >= end_idx {
        let pos = lines
            .get(start_idx)
            .map(|line| line.start)
            .unwrap_or(content_len);
        return (pos, pos);
    }

    let start = lines
        .get(start_idx)
        .map(|line| line.start)
        .unwrap_or(content_len);
    let end_line = end_idx.saturating_sub(1);
    let end = lines
        .get(end_line)
        .map(|line| line.end)
        .unwrap_or(content_len);

    (start, end)
}

fn tokenize_non_whitespace(
    content: &str,
    range: (usize, usize),
    starting_line: usize,
) -> Vec<Token> {
    let (start, end) = range;
    if start >= end {
        return Vec::new();
    }

    let mut tokens = Vec::new();
    let mut line = starting_line;

    for (offset, ch) in content[start..end].char_indices() {
        let abs = start + offset;
        if ch.is_whitespace() {
            if ch == '\n' {
                line += 1;
            }
            continue;
        }

        let mut buf = [0u8; 4];
        let lexeme = ch.encode_utf8(&mut buf).to_string();
        let ch_end = abs + ch.len_utf8();
        tokens.push(Token {
            lexeme,
            start: abs,
            end: ch_end,
            line,
        });
    }

    tokens
}

fn append_range_diffs(
    diffs: &mut Vec<Diff<u8>>,
    old_content: &str,
    new_content: &str,
    old_range: (usize, usize),
    new_range: (usize, usize),
    force_split: bool,
) {
    let (old_start, old_end) = old_range;
    let (new_start, new_end) = new_range;

    if old_start >= old_end && new_start >= new_end {
        return;
    }

    let old_slice = &old_content[old_start..old_end];
    let new_slice = &new_content[new_start..new_end];

    if !force_split && !old_slice.is_empty() && !new_slice.is_empty() && old_slice == new_slice {
        diffs.push(Diff::<u8>::new(Ops::Equal, new_slice.as_bytes()));
        return;
    }

    if !old_slice.is_empty() {
        diffs.push(Diff::<u8>::new(Ops::Delete, old_slice.as_bytes()));
    }
    if !new_slice.is_empty() {
        diffs.push(Diff::<u8>::new(Ops::Insert, new_slice.as_bytes()));
    }
}

fn build_token_aligned_diffs(
    old_content: &str,
    new_content: &str,
    old_range: (usize, usize),
    new_range: (usize, usize),
    old_start_line: usize,
    new_start_line: usize,
) -> (Vec<Diff<u8>>, Vec<(usize, usize)>) {
    let (old_start, old_end) = old_range;
    let (new_start, new_end) = new_range;

    let mut diffs = Vec::new();
    let mut substantive_ranges = Vec::new();

    let old_tokens = tokenize_non_whitespace(old_content, old_range, old_start_line);
    let new_tokens = tokenize_non_whitespace(new_content, new_range, new_start_line);

    if old_tokens.is_empty() && new_tokens.is_empty() {
        append_range_diffs(
            &mut diffs,
            old_content,
            new_content,
            (old_start, old_end),
            (new_start, new_end),
            false,
        );
        return (diffs, substantive_ranges);
    }

    let token_ops = capture_diff_slices(Algorithm::Myers, &old_tokens, &new_tokens);
    let mut old_cursor = old_start;
    let mut new_cursor = new_start;
    let mut last_was_change = false;

    for op in token_ops {
        match op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                for i in 0..len {
                    let old_token = &old_tokens[old_index + i];
                    let new_token = &new_tokens[new_index + i];

                    append_range_diffs(
                        &mut diffs,
                        old_content,
                        new_content,
                        (old_cursor, old_token.start),
                        (new_cursor, new_token.start),
                        last_was_change,
                    );

                    diffs.push(Diff::<u8>::new(
                        Ops::Equal,
                        new_content[new_token.start..new_token.end].as_bytes(),
                    ));

                    old_cursor = old_token.end;
                    new_cursor = new_token.end;
                    last_was_change = false;
                }
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                if old_len == 0 {
                    continue;
                }

                let start = old_tokens[old_index].start;
                let end = old_tokens[old_index + old_len - 1].end;

                append_range_diffs(
                    &mut diffs,
                    old_content,
                    new_content,
                    (old_cursor, start),
                    (new_cursor, new_cursor),
                    last_was_change,
                );

                diffs.push(Diff::<u8>::new(
                    Ops::Delete,
                    old_content[start..end].as_bytes(),
                ));

                old_cursor = end;
                last_was_change = true;
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                if new_len == 0 {
                    continue;
                }

                let start = new_tokens[new_index].start;
                let end = new_tokens[new_index + new_len - 1].end;

                append_range_diffs(
                    &mut diffs,
                    old_content,
                    new_content,
                    (old_cursor, old_cursor),
                    (new_cursor, start),
                    last_was_change,
                );

                diffs.push(Diff::<u8>::new(
                    Ops::Insert,
                    new_content[start..end].as_bytes(),
                ));

                substantive_ranges.push((start, end));
                new_cursor = end;
                last_was_change = true;
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                let old_start_pos = old_tokens
                    .get(old_index)
                    .map(|t| t.start)
                    .unwrap_or(old_cursor);
                let new_start_pos = new_tokens
                    .get(new_index)
                    .map(|t| t.start)
                    .unwrap_or(new_cursor);

                append_range_diffs(
                    &mut diffs,
                    old_content,
                    new_content,
                    (old_cursor, old_start_pos),
                    (new_cursor, new_start_pos),
                    last_was_change,
                );

                if old_len > 0 {
                    let old_end_pos = old_tokens[old_index + old_len - 1].end;
                    diffs.push(Diff::<u8>::new(
                        Ops::Delete,
                        old_content[old_start_pos..old_end_pos].as_bytes(),
                    ));
                    old_cursor = old_end_pos;
                } else {
                    old_cursor = old_start_pos;
                }

                if new_len > 0 {
                    let new_end_pos = new_tokens[new_index + new_len - 1].end;
                    diffs.push(Diff::<u8>::new(
                        Ops::Insert,
                        new_content[new_start_pos..new_end_pos].as_bytes(),
                    ));
                    substantive_ranges.push((new_start_pos, new_end_pos));
                    new_cursor = new_end_pos;
                } else {
                    new_cursor = new_start_pos;
                }
                last_was_change = true;
            }
        }
    }

    append_range_diffs(
        &mut diffs,
        old_content,
        new_content,
        (old_cursor, old_end),
        (new_cursor, new_end),
        last_was_change,
    );

    (diffs, substantive_ranges)
}

fn merge_ranges(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    if ranges.is_empty() {
        return ranges;
    }

    ranges.sort_by_key(|r| (r.0, r.1));
    let mut merged: Vec<(usize, usize)> = Vec::new();

    for (start, end) in ranges {
        if start >= end {
            continue;
        }

        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
            } else {
                merged.push((start, end));
            }
        } else {
            merged.push((start, end));
        }
    }

    merged
}

fn ranges_intersect(ranges: &[(usize, usize)], target: (usize, usize)) -> bool {
    let (start, end) = target;
    if start >= end {
        return false;
    }

    for &(r_start, r_end) in ranges {
        if r_end <= start {
            continue;
        }
        if r_start >= end {
            return false;
        }
        return true;
    }

    false
}

fn find_attribution_for_insertion<'a>(
    old_attributions: &'a [Attribution],
    position: usize,
) -> Option<&'a Attribution> {
    if let Some(overlapping) = old_attributions
        .iter()
        .filter(|a| a.overlaps(position, position.saturating_add(1)))
        .max_by(|a, b| a.ts.cmp(&b.ts).then_with(|| (a.end - a.start).cmp(&(b.end - b.start))))
    {
        return Some(overlapping);
    }

    let before = old_attributions
        .iter()
        .filter(|a| a.end <= position)
        .max_by_key(|a| a.end);
    let after = old_attributions
        .iter()
        .filter(|a| a.start >= position)
        .min_by_key(|a| a.start);

    before.or(after)
}

impl Default for AttributionTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper struct to track line boundaries in content
struct LineBoundaries {
    /// Maps line number (1-indexed) to (start_char, end_char) exclusive end
    line_ranges: Vec<(usize, usize)>,
}

impl LineBoundaries {
    fn new(content: &str) -> Self {
        let mut line_ranges = Vec::new();
        let mut start = 0;

        for (idx, _) in content.match_indices('\n') {
            // Line from start to idx (inclusive of newline)
            line_ranges.push((start, idx + 1));
            start = idx + 1;
        }

        // Handle last line if it doesn't end with newline
        if start < content.len() {
            line_ranges.push((start, content.len()));
        } else if start == content.len() && content.is_empty() {
            // Empty file - no lines
        } else if start == content.len() && !content.is_empty() {
            // File ends with newline, last line is already added
        }

        LineBoundaries { line_ranges }
    }

    fn line_count(&self) -> u32 {
        self.line_ranges.len() as u32
    }

    fn get_line_range(&self, line_num: u32) -> Option<(usize, usize)> {
        if line_num < 1 || line_num as usize > self.line_ranges.len() {
            None
        } else {
            Some(self.line_ranges[line_num as usize - 1])
        }
    }
}

/// Convert line-based attributions to character-based attributions.
///
/// # Arguments
/// * `line_attributions` - Line-based attributions to convert
/// * `content` - The file content to map line numbers to character positions
///
/// # Returns
/// A vector of character-based attributions covering the same ranges
pub fn line_attributions_to_attributions(
    line_attributions: &Vec<LineAttribution>,
    content: &str,
    ts: u128,
) -> Vec<Attribution> {
    if line_attributions.is_empty() || content.is_empty() {
        return Vec::new();
    }

    let boundaries = LineBoundaries::new(content);
    let mut result = Vec::new();

    for line_attr in line_attributions {
        // Get character ranges for start and end lines
        let start_range = boundaries.get_line_range(line_attr.start_line);
        let end_range = boundaries.get_line_range(line_attr.end_line);

        if let (Some((start_char, _)), Some((_, end_char))) = (start_range, end_range) {
            result.push(Attribution::new(
                start_char,
                end_char,
                line_attr.author_id.clone(),
                ts,
            ));
        }
    }

    result
}

/// Convert character-based attributions to line-based attributions.
/// For each line, selects the "dominant" author based on who contributed
/// the most non-whitespace characters to that line.
/// Finally, strip away all human-authored lines that aren't overrides.
///
/// # Arguments
/// * `attributions` - Character-based attributions
/// * `content` - The file content being attributed
///
/// # Returns
/// A vector of line attributions with consecutive lines by the same author merged
pub fn attributions_to_line_attributions(
    attributions: &Vec<Attribution>,
    content: &str,
) -> Vec<LineAttribution> {
    if content.is_empty() || attributions.is_empty() {
        return Vec::new();
    }

    let boundaries = LineBoundaries::new(content);
    let line_count = boundaries.line_count();

    if line_count == 0 {
        return Vec::new();
    }

    // For each line, determine the dominant author
    let mut line_authors: Vec<Option<(String, Option<String>)>> =
        Vec::with_capacity(line_count as usize);

    for line_num in 1..=line_count {
        let (author, overrode) =
            find_dominant_author_for_line(line_num, &boundaries, attributions, content);
        line_authors.push(Some((author, overrode)));
    }

    // Merge consecutive lines with the same author
    let mut merged_line_authors = merge_consecutive_line_attributions(line_authors);

    // Strip away all human lines (only AI lines need to be retained)
    merged_line_authors.retain(|line_attr| {
        line_attr.author_id != CheckpointKind::Human.to_str() || line_attr.overrode.is_some()
    });
    merged_line_authors
}

/// Find the dominant author for a specific line based on non-whitespace character count
fn find_dominant_author_for_line(
    line_num: u32,
    boundaries: &LineBoundaries,
    attributions: &Vec<Attribution>,
    full_content: &str,
) -> (String, Option<String>) {
    let (line_start, line_end) = boundaries.get_line_range(line_num).unwrap();
    let line_content = &full_content[line_start..line_end];
    let is_line_empty = line_content.is_empty() || line_content.chars().all(|c| c.is_whitespace());

    let mut candidate_attrs = Vec::new();
    for attribution in attributions {
        if !attribution.overlaps(line_start, line_end) {
            continue;
        }

        // Get the substring of the content on this line that is covered by the attribution
        let content_slice = &full_content[std::cmp::max(line_start, attribution.start)
            ..std::cmp::min(line_end, attribution.end)];
        let attr_non_whitespace_count =
            content_slice.chars().filter(|c| !c.is_whitespace()).count();
        if attr_non_whitespace_count > 0 || is_line_empty {
            candidate_attrs.push(attribution.clone());
        } else {
            // If the attribution is only whitespace, discard it
            continue;
        }
    }

    if candidate_attrs.is_empty() {
        return (CheckpointKind::Human.to_str(), None);
    }

    // Choose the author with the latest timestamp
    let latest_timestamp = candidate_attrs.iter().max_by_key(|a| a.ts).unwrap().ts;
    let latest_author = candidate_attrs
        .iter()
        .filter(|a| a.ts == latest_timestamp)
        .map(|a| a.author_id.clone())
        .collect::<Vec<String>>();
    let last_ai_edit = candidate_attrs
        .iter()
        .filter(|a| a.author_id != CheckpointKind::Human.to_str())
        .last();
    let last_human_edit = candidate_attrs
        .iter()
        .filter(|a| a.author_id == CheckpointKind::Human.to_str())
        .last();
    let overrode = match (last_ai_edit, last_human_edit) {
        (Some(ai), Some(h)) => {
            if h.ts > ai.ts {
                Some(ai.author_id.clone())
            } else {
                None
            }
        }
        _ => None,
    };
    return (latest_author[0].clone(), overrode);
}

/// Merge consecutive lines with the same author into LineAttribution ranges
fn merge_consecutive_line_attributions(
    line_authorship: Vec<Option<(String, Option<String>)>>,
) -> Vec<LineAttribution> {
    let mut result = Vec::new();
    let line_count = line_authorship.len();

    let mut current_authorship: Option<(String, Option<String>)> = None;
    let mut current_start: u32 = 0;

    for (idx, authorship) in line_authorship.into_iter().enumerate() {
        let line_num = (idx + 1) as u32;

        match (&current_authorship, authorship) {
            (None, None) => {
                // No attribution for this line, continue
            }
            (None, Some(new_author)) => {
                // Start a new line attribution
                current_authorship = Some(new_author);
                current_start = line_num;
            }
            (Some(_), None) => {
                // End current attribution
                if let Some(authorship) = current_authorship.take() {
                    result.push(LineAttribution::new(
                        current_start,
                        line_num - 1,
                        authorship.0,
                        authorship.1,
                    ));
                }
            }
            (Some(curr), Some(new_authorship)) => {
                if curr == &new_authorship {
                    // Continue current attribution
                } else {
                    // End current, start new
                    result.push(LineAttribution::new(
                        current_start,
                        line_num - 1,
                        curr.0.clone(),
                        curr.1.clone(),
                    ));
                    current_authorship = Some(new_authorship);
                    current_start = line_num;
                }
            }
        }
    }

    // Close final attribution if any
    if let Some(authorship) = current_authorship {
        result.push(LineAttribution::new(
            current_start,
            line_count as u32,
            authorship.0,
            authorship.1,
        ));
    }

    result
}
#[cfg(test)]
mod tests {
    use super::*;

    const TEST_TS: u128 = 1234567890000;

    fn assert_range_owned_by(attributions: &[Attribution], start: usize, end: usize, author: &str) {
        assert!(start < end, "expected non-empty range");
        let owner = attributions
            .iter()
            .find(|a| a.start <= start && a.end >= end)
            .unwrap_or_else(|| panic!("range {}..{} missing in {:?}", start, end, attributions));
        assert_eq!(
            owner.author_id, author,
            "expected {} to own {}..{}, got {}",
            author, start, end, owner.author_id
        );
    }

    fn assert_non_ws_owned_by(
        attributions: &[Attribution],
        content: &str,
        author: &str,
        message: &str,
    ) {
        for (idx, ch) in content.char_indices() {
            if ch.is_whitespace() {
                continue;
            }
            let owner = attributions.iter().find(|a| a.start <= idx && a.end > idx);
            assert!(
                owner.map(|a| a.author_id.as_str()) == Some(author),
                "{}: non-ws char '{}' at {} owned by {:?}",
                message,
                ch,
                idx,
                owner.map(|a| a.author_id.as_str())
            );
        }
    }

    #[test]
    fn substantive_token_change_switches_author() {
        let tracker = AttributionTracker::new();
        let old = "fn main() {\n    let value = 1;\n}\n";
        let new = "fn main() {\n    let value = 2;\n}\n";
        let old_attrs = vec![Attribution::new(0, old.len(), "Alice".into(), TEST_TS)];

        let updated = tracker
            .update_attributions(old, new, &old_attrs, "Bob", TEST_TS + 1)
            .unwrap();

        let two_pos = new.find('2').unwrap();
        assert_range_owned_by(&updated, two_pos, two_pos + 1, "Bob");
        let prefix_end = new.find('1').unwrap_or(two_pos);
        assert_non_ws_owned_by(
            &updated,
            &new[..prefix_end],
            "Alice",
            "unchanged prefix should stay Alice",
        );
    }

    #[test]
    fn whitespace_only_indent_change_preserves_tokens() {
        let tracker = AttributionTracker::new();
        let old = "fn test() {\n  do_stuff();\n}\n";
        let new = "fn test() {\n        do_stuff();\n}\n";
        let old_attrs = vec![Attribution::new(0, old.len(), "Alice".into(), TEST_TS)];

        let updated = tracker
            .update_attributions(old, new, &old_attrs, "Bob", TEST_TS + 1)
            .unwrap();

        assert_non_ws_owned_by(&updated, new, "Alice", "indentation change should not steal tokens");
    }

    #[test]
    fn line_reflow_without_token_change_is_non_substantive() {
        let tracker = AttributionTracker::new();
        let old = "call(foo, bar, baz)";
        let new = "call(\n  foo,\n  bar,\n  baz\n)";
        let old_attrs = vec![Attribution::new(0, old.len(), "Alice".into(), TEST_TS)];

        let updated = tracker
            .update_attributions(old, new, &old_attrs, "Bob", TEST_TS + 1)
            .unwrap();

        let line_attrs = attributions_to_line_attributions(&updated, new);
        assert!(
            line_attrs.iter().all(|la| la.author_id == "Alice"),
            "every reflowed line should remain Alice, got {:?}",
            line_attrs
        );
    }

    #[test]
    fn move_block_preserves_original_authors() {
        let tracker = AttributionTracker::new();
        let old = "fn helper() { println!(\"helper\"); }\nfn main() { println!(\"main\"); }\n";
        let new = "fn main() { println!(\"main\"); }\nfn helper() { println!(\"helper\"); }\n";
        let old_attrs = vec![
            Attribution::new(0, 36, "Alice".into(), TEST_TS),
            Attribution::new(36, old.len(), "Bob".into(), TEST_TS),
        ];

        let updated = tracker
            .update_attributions(old, new, &old_attrs, "Charlie", TEST_TS + 1)
            .unwrap();

        let helper_pos = new.find("helper").unwrap();
        assert_range_owned_by(&updated, helper_pos, helper_pos + "helper".len(), "Alice");
        let main_pos = new.find("main").unwrap();
        assert!(
            updated
                .iter()
                .filter(|a| a.start <= main_pos && a.end >= main_pos + "main".len())
                .any(|a| a.author_id != "Alice"),
            "Moved main block should not be reassigned to helper author"
        );
    }

    #[test]
    fn deletions_remove_attribution() {
        let tracker = AttributionTracker::new();
        let old = "keep remove keep";
        let new = "keep  keep";
        let old_attrs = vec![
            Attribution::new(0, 4, "Alice".into(), TEST_TS),
            Attribution::new(5, 11, "Bob".into(), TEST_TS),
            Attribution::new(12, old.len(), "Alice".into(), TEST_TS),
        ];

        let updated = tracker
            .update_attributions(old, new, &old_attrs, "Carol", TEST_TS + 1)
            .unwrap();

        assert!(
            updated.iter().all(|a| a.author_id != "Bob"),
            "Bob attribution should disappear after deletion"
        );
    }

    #[test]
    fn multibyte_tokens_are_preserved_and_added() {
        let tracker = AttributionTracker::new();
        let old = "😀 one\n";
        let new = "😀 one\n✅ two\n";
        let old_attrs = vec![Attribution::new(0, old.len(), "Alice".into(), TEST_TS)];

        let updated = tracker
            .update_attributions(old, new, &old_attrs, "Bob", TEST_TS + 1)
            .unwrap();

        assert_range_owned_by(&updated, 0, old.len(), "Alice");
        assert!(
            updated
                .iter()
                .any(|a| a.author_id == "Bob" && a.start >= old.len()),
            "New multibyte tokens should belong to Bob"
        );
    }

    #[test]
    fn line_attributions_follow_dominant_tokens() {
        let content = "let x = foo() + bar();\n";
        let attrs = vec![
            Attribution::new(0, 8, "Alice".into(), TEST_TS),
            Attribution::new(8, 13, "Bob".into(), TEST_TS),
            Attribution::new(13, 21, "Carol".into(), TEST_TS),
        ];

        let line_attrs = attributions_to_line_attributions(&attrs, content);
        assert_eq!(line_attrs.len(), 1);
        assert_eq!(line_attrs[0].author_id, "Alice");
    }

    #[test]
    fn unattributed_ranges_are_filled() {
        let tracker = AttributionTracker::new();
        let content = "A B C";
        let prev = vec![Attribution::new(0, 1, "Alice".into(), TEST_TS)];
        let filled = tracker.attribute_unattributed_ranges(content, &prev, "Bob", TEST_TS + 1);

        assert_eq!(filled.len(), 2);
        assert_range_owned_by(&filled, 0, 1, "Alice");
        assert_range_owned_by(&filled, 1, content.len(), "Bob");
    }
}
