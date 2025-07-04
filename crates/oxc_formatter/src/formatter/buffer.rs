#![expect(clippy::mutable_key_type)]
use std::{
    any::{Any, TypeId},
    fmt::Debug,
    ops::{Deref, DerefMut},
};

use rustc_hash::FxHashMap;

use super::{
    Arguments, Format, FormatElement, FormatResult, FormatState,
    format_element::Interned,
    prelude::{LineMode, PrintMode, Tag, tag::Condition},
};
use crate::write;

/// A trait for writing or formatting into `FormatElement`-accepting buffers or streams.
pub trait Buffer<'ast> {
    /// Writes a [crate::FormatElement] into this buffer, returning whether the write succeeded.
    ///
    /// # Errors
    /// This function will return an instance of [crate::FormatError] on error.
    ///
    /// # Examples
    ///
    /// ```
    /// use biome_formatter::{Buffer, FormatElement, FormatState, SimpleFormatContext, VecBuffer};
    ///
    /// let mut state = FormatState::new(SimpleFormatContext::default());
    /// let mut buffer = VecBuffer::new(&mut state);
    ///
    /// buffer.write_element(FormatElement::StaticText { text: "test"}).unwrap();
    ///
    /// assert_eq!(buffer.into_vec(), vec![FormatElement::StaticText { text: "test" }]);
    /// ```
    ///
    fn write_element(&mut self, element: FormatElement<'ast>) -> FormatResult<()>;

    /// Returns a slice containing all elements written into this buffer.
    ///
    /// Prefer using [BufferExtensions::start_recording] over accessing [Buffer::elements] directly.
    #[doc(hidden)]
    fn elements(&self) -> &[FormatElement<'ast>];

    /// Glue for usage of the [`write!`] macro with implementors of this trait.
    ///
    /// This method should generally not be invoked manually, but rather through the [`write!`] macro itself.
    ///
    /// # Errors
    ///
    /// # Examples
    ///
    /// ```
    /// use biome_formatter::prelude::*;
    /// use biome_formatter::{Buffer, FormatState, SimpleFormatContext, VecBuffer, format_args};
    ///
    /// let mut state = FormatState::new(SimpleFormatContext::default());
    /// let mut buffer = VecBuffer::new(&mut state);
    ///
    /// buffer.write_fmt(format_args!(text("Hello World"))).unwrap();
    ///
    /// assert_eq!(buffer.into_vec(), vec![FormatElement::StaticText{ text: "Hello World" }]);
    /// ```
    fn write_fmt(mut self: &mut Self, arguments: Arguments<'_, 'ast>) -> FormatResult<()> {
        super::write(&mut self, arguments)
    }

    /// Returns the formatting state relevant for this formatting session.
    fn state(&self) -> &FormatState<'ast>;

    /// Returns the mutable formatting state relevant for this formatting session.
    fn state_mut(&mut self) -> &mut FormatState<'ast>;

    /// Takes a snapshot of the Buffers state, excluding the formatter state.
    fn snapshot(&self) -> BufferSnapshot;

    /// Restores the snapshot buffer
    ///
    /// ## Panics
    /// If the passed snapshot id is a snapshot of another buffer OR
    /// if the snapshot is restored out of order
    fn restore_snapshot(&mut self, snapshot: BufferSnapshot);
}

/// Snapshot of a buffer state that can be restored at a later point.
///
/// Used in cases where the formatting of an object fails but a parent formatter knows an alternative
/// strategy on how to format the object that might succeed.
#[derive(Debug)]
pub enum BufferSnapshot {
    /// Stores an absolute position of a buffers state, for example, the offset of the last written element.
    Position(usize),

    /// Generic structure for custom buffers that need to store more complex data. Slightly more
    /// expensive because it requires allocating the buffer state on the heap.
    Any(Box<dyn Any>),
}

impl BufferSnapshot {
    /// Creates a new buffer snapshot that points to the specified position.
    pub const fn position(index: usize) -> Self {
        Self::Position(index)
    }

    /// Unwraps the position value.
    ///
    /// # Panics
    ///
    /// If self is not a [`BufferSnapshot::Position`]
    pub fn unwrap_position(&self) -> usize {
        match self {
            BufferSnapshot::Position(index) => *index,
            BufferSnapshot::Any(_) => panic!("Tried to unwrap Any snapshot as a position."),
        }
    }

    /// Unwraps the any value.
    ///
    /// # Panics
    ///
    /// If `self` is not a [`BufferSnapshot::Any`].
    pub fn unwrap_any<T: 'static>(self) -> T {
        match self {
            BufferSnapshot::Position(_) => {
                panic!("Tried to unwrap Position snapshot as Any snapshot.")
            }
            BufferSnapshot::Any(value) => match value.downcast::<T>() {
                Ok(snapshot) => *snapshot,
                Err(err) => {
                    panic!(
                        "Tried to unwrap snapshot of type {:?} as {:?}",
                        (*err).type_id(),
                        TypeId::of::<T>()
                    )
                }
            },
        }
    }
}

/// Implements the `[Buffer]` trait for all mutable references of objects implementing [Buffer].
impl<'ast, W: Buffer<'ast> + ?Sized> Buffer<'ast> for &mut W {
    fn write_element(&mut self, element: FormatElement<'ast>) -> FormatResult<()> {
        (**self).write_element(element)
    }

    fn elements(&self) -> &[FormatElement<'ast>] {
        (**self).elements()
    }

    fn write_fmt(&mut self, args: Arguments<'_, 'ast>) -> FormatResult<()> {
        (**self).write_fmt(args)
    }

    fn state(&self) -> &FormatState<'ast> {
        (**self).state()
    }

    fn state_mut(&mut self) -> &mut FormatState<'ast> {
        (**self).state_mut()
    }

    fn snapshot(&self) -> BufferSnapshot {
        (**self).snapshot()
    }

    fn restore_snapshot(&mut self, snapshot: BufferSnapshot) {
        (**self).restore_snapshot(snapshot);
    }
}

/// Vector backed [`Buffer`] implementation.
///
/// The buffer writes all elements into the internal elements buffer.
#[derive(Debug)]
pub struct VecBuffer<'buf, 'ast> {
    state: &'buf mut FormatState<'ast>,
    elements: Vec<FormatElement<'ast>>,
}

impl<'buf, 'ast> VecBuffer<'buf, 'ast> {
    pub fn new(state: &'buf mut FormatState<'ast>) -> Self {
        Self::new_with_vec(state, Vec::new())
    }

    pub fn new_with_vec(
        state: &'buf mut FormatState<'ast>,
        elements: Vec<FormatElement<'ast>>,
    ) -> Self {
        Self { state, elements }
    }

    /// Creates a buffer with the specified capacity
    pub fn with_capacity(capacity: usize, state: &'buf mut FormatState<'ast>) -> Self {
        Self { state, elements: Vec::with_capacity(capacity) }
    }

    /// Consumes the buffer and returns the written [`FormatElement]`s as a vector.
    pub fn into_vec(self) -> Vec<FormatElement<'ast>> {
        self.elements
    }

    /// Takes the elements without consuming self
    pub fn take_vec(&mut self) -> Vec<FormatElement<'ast>> {
        std::mem::take(&mut self.elements)
    }
}

impl<'ast> Deref for VecBuffer<'_, 'ast> {
    type Target = [FormatElement<'ast>];

    fn deref(&self) -> &Self::Target {
        &self.elements
    }
}

impl DerefMut for VecBuffer<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.elements
    }
}

impl<'ast> Buffer<'ast> for VecBuffer<'_, 'ast> {
    fn write_element(&mut self, element: FormatElement<'ast>) -> FormatResult<()> {
        self.elements.push(element);

        Ok(())
    }

    fn elements(&self) -> &[FormatElement<'ast>] {
        self
    }

    fn state(&self) -> &FormatState<'ast> {
        self.state
    }

    fn state_mut(&mut self) -> &mut FormatState<'ast> {
        self.state
    }

    fn snapshot(&self) -> BufferSnapshot {
        BufferSnapshot::position(self.elements.len())
    }

    fn restore_snapshot(&mut self, snapshot: BufferSnapshot) {
        let position = snapshot.unwrap_position();
        assert!(
            self.elements.len() >= position,
            r"Outdated snapshot. This buffer contains fewer elements than at the time the snapshot was taken.
Make sure that you take and restore the snapshot in order and that this snapshot belongs to the current buffer."
        );

        self.elements.truncate(position);
    }
}

/// This struct wraps an existing buffer and emits a preamble text when the first text is written.
///
/// This can be useful if you, for example, want to write some content if what gets written next isn't empty.
///
/// # Examples
///
/// ```
/// use biome_formatter::{FormatState, Formatted, PreambleBuffer, SimpleFormatContext, VecBuffer, write};
/// use biome_formatter::prelude::*;
///
/// struct Preamble;
///
/// impl Format<SimpleFormatContext> for Preamble {
///     fn fmt(&self, f: &mut Formatter<SimpleFormatContext>) -> FormatResult<()> {
///         write!(f, [text("# heading"), hard_line_break()])
///     }
/// }
///
/// # fn main() -> FormatResult<()> {
/// let mut state = FormatState::new(SimpleFormatContext::default());
/// let mut buffer = VecBuffer::new(&mut state);
///
/// {
///     let mut with_preamble = PreambleBuffer::new(&mut buffer, Preamble);
///
///     write!(&mut with_preamble, [text("this text will be on a new line")])?;
/// }
///
/// let formatted = Formatted::new(Document::from(buffer.into_vec()), SimpleFormatContext::default());
/// assert_eq!("# heading\nthis text will be on a new line", formatted.print()?.as_code());
///
/// # Ok(())
/// # }
/// ```
///
/// The pre-amble does not get written if no content is written to the buffer.
///
/// ```
/// use biome_formatter::{FormatState, Formatted, PreambleBuffer, SimpleFormatContext, VecBuffer, write};
/// use biome_formatter::prelude::*;
///
/// struct Preamble;
///
/// impl Format<SimpleFormatContext> for Preamble {
///     fn fmt(&self, f: &mut Formatter<SimpleFormatContext>) -> FormatResult<()> {
///         write!(f, [text("# heading"), hard_line_break()])
///     }
/// }
///
/// # fn main() -> FormatResult<()> {
/// let mut state = FormatState::new(SimpleFormatContext::default());
/// let mut buffer = VecBuffer::new(&mut state);
/// {
///     let mut with_preamble = PreambleBuffer::new(&mut buffer, Preamble);
/// }
///
/// let formatted = Formatted::new(Document::from(buffer.into_vec()), SimpleFormatContext::default());
/// assert_eq!("", formatted.print()?.as_code());
/// # Ok(())
/// # }
/// ```
pub struct PreambleBuffer<'a, 'buf, Preamble> {
    /// The wrapped buffer
    inner: &'buf mut dyn Buffer<'a>,

    /// The pre-amble to write once the first content gets written to this buffer.
    preamble: Preamble,

    /// Whether some content (including the pre-amble) has been written at this point.
    empty: bool,
}

impl<'ast, 'buf, Preamble> PreambleBuffer<'ast, 'buf, Preamble> {
    pub fn new(inner: &'buf mut dyn Buffer<'ast>, preamble: Preamble) -> Self {
        Self { inner, preamble, empty: true }
    }

    /// Returns `true` if the preamble has been written, `false` otherwise.
    pub fn did_write_preamble(&self) -> bool {
        !self.empty
    }
}

impl<'ast, Preamble> Buffer<'ast> for PreambleBuffer<'ast, '_, Preamble>
where
    Preamble: Format<'ast>,
{
    fn write_element(&mut self, element: FormatElement<'ast>) -> FormatResult<()> {
        if self.empty {
            write!(self.inner, [&self.preamble])?;
            self.empty = false;
        }

        self.inner.write_element(element)
    }

    fn elements(&self) -> &[FormatElement<'ast>] {
        self.inner.elements()
    }

    fn state(&self) -> &FormatState<'ast> {
        self.inner.state()
    }

    fn state_mut(&mut self) -> &mut FormatState<'ast> {
        self.inner.state_mut()
    }

    fn snapshot(&self) -> BufferSnapshot {
        BufferSnapshot::Any(Box::new(PreambleBufferSnapshot {
            inner: self.inner.snapshot(),
            empty: self.empty,
        }))
    }

    fn restore_snapshot(&mut self, snapshot: BufferSnapshot) {
        let snapshot = snapshot.unwrap_any::<PreambleBufferSnapshot>();

        self.empty = snapshot.empty;
        self.inner.restore_snapshot(snapshot.inner);
    }
}

struct PreambleBufferSnapshot {
    inner: BufferSnapshot,
    empty: bool,
}

/// Buffer that allows you inspecting elements as they get written to the formatter.
pub struct Inspect<'ast, 'inner, Inspector> {
    inner: &'inner mut dyn Buffer<'ast>,
    inspector: Inspector,
}

impl<'ast, 'inner, Inspector> Inspect<'ast, 'inner, Inspector> {
    fn new(inner: &'inner mut dyn Buffer<'ast>, inspector: Inspector) -> Self {
        Self { inner, inspector }
    }
}

impl<'a, Inspector> Buffer<'a> for Inspect<'a, '_, Inspector>
where
    Inspector: FnMut(&FormatElement),
{
    fn write_element(&mut self, element: FormatElement<'a>) -> FormatResult<()> {
        (self.inspector)(&element);
        self.inner.write_element(element)
    }

    fn elements(&self) -> &[FormatElement<'a>] {
        self.inner.elements()
    }

    fn state(&self) -> &FormatState<'a> {
        self.inner.state()
    }

    fn state_mut(&mut self) -> &mut FormatState<'a> {
        self.inner.state_mut()
    }

    fn snapshot(&self) -> BufferSnapshot {
        self.inner.snapshot()
    }

    fn restore_snapshot(&mut self, snapshot: BufferSnapshot) {
        self.inner.restore_snapshot(snapshot);
    }
}

/// A Buffer that removes any soft line breaks.
///
/// * Removes [`lines`](FormatElement::Line) with the mode [`Soft`](LineMode::Soft).
/// * Replaces [`lines`](FormatElement::Line) with the mode [`Soft`](LineMode::SoftOrSpace) with a [`Space`](FormatElement::Space)
///
/// # Examples
///
/// ```
/// use biome_formatter::prelude::*;
/// use biome_formatter::{format, write};
///
/// # fn main() -> FormatResult<()> {
/// use biome_formatter::{RemoveSoftLinesBuffer, SimpleFormatContext, VecBuffer};
/// use biome_formatter::prelude::format_with;
/// let formatted = format!(
///     SimpleFormatContext::default(),
///     [format_with(|f| {
///         let mut buffer = RemoveSoftLinesBuffer::new(f);
///
///         write!(
///             buffer,
///             [
///                 text("The next soft line or space gets replaced by a space"),
///                 soft_line_break_or_space(),
///                 text("and the line here"),
///                 soft_line_break(),
///                 text("is removed entirely.")
///             ]
///         )
///     })]
/// )?;
///
/// assert_eq!(
///     formatted.document().as_ref(),
///     &[
///         FormatElement::StaticText { text: "The next soft line or space gets replaced by a space" },
///         FormatElement::Space,
///         FormatElement::StaticText { text: "and the line here" },
///         FormatElement::StaticText { text: "is removed entirely." }
///     ]
/// );
///
/// # Ok(())
/// # }
/// ```
pub struct RemoveSoftLinesBuffer<'buf, 'ast> {
    inner: &'buf mut dyn Buffer<'ast>,

    /// Caches the interned elements after the soft line breaks have been removed.
    ///
    /// The `key` is the [Interned] element as it has been passed to [Self::write_element] or the child of another
    /// [Interned] element. The `value` is the matching document of the key where all soft line breaks have been removed.
    ///
    /// It's fine to not snapshot the cache. The worst that can happen is that it holds on interned elements
    /// that are now unused. But there's little harm in that and the cache is cleaned when dropping the buffer.
    interned_cache: FxHashMap<Interned<'ast>, Interned<'ast>>,

    /// Store the conditional content stack to help determine if the current element is within expanded conditional content.
    conditional_content_stack: Vec<Condition>,
}

impl<'buf, 'ast> RemoveSoftLinesBuffer<'buf, 'ast> {
    /// Creates a new buffer that removes the soft line breaks before writing them into `buffer`.
    pub fn new(inner: &'buf mut dyn Buffer<'ast>) -> Self {
        Self { inner, interned_cache: FxHashMap::default(), conditional_content_stack: Vec::new() }
    }

    /// Removes the soft line breaks from an interned element.
    fn clean_interned(&mut self, interned: &Interned<'ast>) -> Interned<'ast> {
        clean_interned(interned, &mut self.interned_cache, &mut self.conditional_content_stack)
    }

    /// Marker for whether a `StartConditionalContent(mode: Expanded)` has been
    /// written but not yet closed.
    fn is_in_expanded_conditional_content(&self) -> bool {
        self.conditional_content_stack
            .iter()
            .last()
            .is_some_and(|condition| condition.mode == PrintMode::Expanded)
    }
}

// Extracted to function to avoid monomorphization
fn clean_interned<'ast>(
    interned: &Interned<'ast>,
    interned_cache: &mut FxHashMap<Interned<'ast>, Interned<'ast>>,
    condition_content_stack: &mut Vec<Condition>,
) -> Interned<'ast> {
    if let Some(cleaned) = interned_cache.get(interned) {
        cleaned.clone()
    } else {
        // Find the first soft line break element, interned element, or conditional expanded
        // content that must be changed.
        let result = interned.iter().enumerate().find_map(|(index, element)| match element {
            FormatElement::Line(LineMode::Soft | LineMode::SoftOrSpace)
            | FormatElement::Tag(Tag::StartConditionalContent(_) | Tag::EndConditionalContent)
            | FormatElement::BestFitting(_) => {
                let mut cleaned = Vec::new();
                cleaned.extend_from_slice(&interned[..index]);
                Some((cleaned, &interned[index..]))
            }
            FormatElement::Interned(inner) => {
                let cleaned_inner = clean_interned(inner, interned_cache, condition_content_stack);

                if &cleaned_inner == inner {
                    None
                } else {
                    let mut cleaned = Vec::with_capacity(interned.len());
                    cleaned.extend_from_slice(&interned[..index]);
                    cleaned.push(FormatElement::Interned(cleaned_inner));
                    Some((cleaned, &interned[index + 1..]))
                }
            }
            _ => None,
        });

        let result = match result {
            // Copy the whole interned buffer so that becomes possible to change the necessary elements.
            Some((mut cleaned, rest)) => {
                let mut element_stack = rest.iter().rev().collect::<Vec<_>>();
                while let Some(element) = element_stack.pop() {
                    match element {
                        FormatElement::Tag(Tag::StartConditionalContent(condition)) => {
                            condition_content_stack.push(condition.clone());
                        }
                        FormatElement::Tag(Tag::EndConditionalContent) => {
                            condition_content_stack.pop();
                        }
                        // All content within an expanded conditional gets dropped. If there's a
                        // matching flat variant, that will still get kept.
                        _ if condition_content_stack
                            .iter()
                            .last()
                            .is_some_and(|condition| condition.mode == PrintMode::Expanded) => {}

                        FormatElement::Line(LineMode::Soft) => {}
                        FormatElement::Line(LineMode::SoftOrSpace) => {
                            cleaned.push(FormatElement::Space);
                        }

                        FormatElement::Interned(interned) => {
                            cleaned.push(FormatElement::Interned(clean_interned(
                                interned,
                                interned_cache,
                                condition_content_stack,
                            )));
                        }
                        // Since this buffer aims to simulate infinite print width, we don't need to retain the best fitting.
                        // Just extract the flattest variant and then handle elements within it.
                        FormatElement::BestFitting(best_fitting) => {
                            let most_flat = best_fitting.most_flat();
                            most_flat.iter().rev().for_each(|element| element_stack.push(element));
                        }
                        element => cleaned.push(element.clone()),
                    }
                }

                Interned::new(cleaned)
            }
            // No change necessary, return existing interned element
            None => interned.clone(),
        };

        interned_cache.insert(interned.clone(), result.clone());
        result
    }
}

impl<'ast> Buffer<'ast> for RemoveSoftLinesBuffer<'_, 'ast> {
    fn write_element(&mut self, element: FormatElement<'ast>) -> FormatResult<()> {
        let mut element_stack = Vec::new();
        element_stack.push(element);
        while let Some(element) = element_stack.pop() {
            match element {
                FormatElement::Tag(Tag::StartConditionalContent(condition)) => {
                    self.conditional_content_stack.push(condition.clone());
                }
                FormatElement::Tag(Tag::EndConditionalContent) => {
                    self.conditional_content_stack.pop();
                }
                // All content within an expanded conditional gets dropped. If there's a
                // matching flat variant, that will still get kept.
                _ if self.is_in_expanded_conditional_content() => {}

                FormatElement::Line(LineMode::Soft) => {}
                FormatElement::Line(LineMode::SoftOrSpace) => {
                    self.inner.write_element(FormatElement::Space)?;
                }
                FormatElement::Interned(interned) => {
                    let cleaned = self.clean_interned(&interned);
                    self.inner.write_element(FormatElement::Interned(cleaned))?;
                }
                // Since this buffer aims to simulate infinite print width, we don't need to retain the best fitting.
                // Just extract the flattest variant and then handle elements within it.
                FormatElement::BestFitting(best_fitting) => {
                    let most_flat = best_fitting.most_flat();
                    most_flat.iter().rev().for_each(|element| element_stack.push(element.clone()));
                }
                element => self.inner.write_element(element)?,
            }
        }
        Ok(())
    }

    fn elements(&self) -> &[FormatElement<'ast>] {
        self.inner.elements()
    }

    fn state(&self) -> &FormatState<'ast> {
        self.inner.state()
    }

    fn state_mut(&mut self) -> &mut FormatState<'ast> {
        self.inner.state_mut()
    }

    fn snapshot(&self) -> BufferSnapshot {
        self.inner.snapshot()
    }

    fn restore_snapshot(&mut self, snapshot: BufferSnapshot) {
        self.inner.restore_snapshot(snapshot);
    }
}

pub trait BufferExtensions<'ast>: Buffer<'ast> + Sized {
    /// Returns a new buffer that calls the passed inspector for every element that gets written to the output
    #[must_use]
    fn inspect<'inner, F>(&'inner mut self, inspector: F) -> Inspect<'ast, 'inner, F>
    where
        F: FnMut(&FormatElement),
    {
        Inspect::new(self, inspector)
    }

    /// Starts a recording that gives you access to all elements that have been written between the start
    /// and end of the recording
    ///
    /// #Examples
    ///
    /// ```
    /// use std::ops::Deref;
    /// use biome_formatter::prelude::*;
    /// use biome_formatter::{write, format, SimpleFormatContext};
    ///
    /// # fn main() -> FormatResult<()> {
    /// let formatted = format!(SimpleFormatContext::default(), [format_with(|f| {
    ///     let mut recording = f.start_recording();
    ///
    ///     write!(recording, [text("A")])?;
    ///     write!(recording, [text("B")])?;
    ///
    ///     write!(recording, [format_with(|f| write!(f, [text("C"), text("D")]))])?;
    ///
    ///     let recorded = recording.stop();
    ///     assert_eq!(
    ///         recorded.deref(),
    ///         &[
    ///             FormatElement::StaticText{ text: "A" },
    ///             FormatElement::StaticText{ text: "B" },
    ///             FormatElement::StaticText{ text: "C" },
    ///             FormatElement::StaticText{ text: "D" }
    ///         ]
    ///     );
    ///
    ///     Ok(())
    /// })])?;
    ///
    /// assert_eq!(formatted.print()?.as_code(), "ABCD");
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    fn start_recording(&mut self) -> Recording<'_, Self> {
        Recording::new(self)
    }

    /// Writes a sequence of elements into this buffer.
    fn write_elements<I>(&mut self, elements: I) -> FormatResult<()>
    where
        I: IntoIterator<Item = FormatElement<'ast>>,
    {
        for element in elements {
            self.write_element(element)?;
        }

        Ok(())
    }
}

impl<'ast, T> BufferExtensions<'ast> for T where T: Buffer<'ast> {}

#[derive(Debug)]
pub struct Recording<'buf, Buffer> {
    start: usize,
    buffer: &'buf mut Buffer,
}

impl<'ast, 'buf, B> Recording<'buf, B>
where
    B: Buffer<'ast>,
{
    fn new(buffer: &'buf mut B) -> Self {
        Self { start: buffer.elements().len(), buffer }
    }

    #[inline(always)]
    pub fn write_fmt(&mut self, arguments: Arguments<'_, 'ast>) -> FormatResult<()> {
        self.buffer.write_fmt(arguments)
    }

    #[inline(always)]
    pub fn write_element(&mut self, element: FormatElement<'ast>) -> FormatResult<()> {
        self.buffer.write_element(element)
    }

    pub fn stop(self) -> Recorded<'buf, 'ast> {
        let buffer: &'buf B = self.buffer;
        let elements = buffer.elements();

        let recorded = if self.start > elements.len() {
            // May happen if buffer was rewinded.
            &[]
        } else {
            &elements[self.start..]
        };

        Recorded(recorded)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Recorded<'buf, 'ast>(&'buf [FormatElement<'ast>]);

impl<'ast> Deref for Recorded<'_, 'ast> {
    type Target = [FormatElement<'ast>];

    fn deref(&self) -> &Self::Target {
        self.0
    }
}
