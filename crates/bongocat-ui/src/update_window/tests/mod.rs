//! The update window's tests, split by what they cover.
//!
//! The unit tests ask about the window's arithmetic and its strings; the render
//! tests build a real frame, which is the only way to ask whether a phase
//! actually offers the action it promises. Each names what it needs: the two
//! have almost nothing in common, and a shared prelude would be a list of names
//! half of which is unused in either.

mod render;
mod unit;
