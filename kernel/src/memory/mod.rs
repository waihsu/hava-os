//! Memory-management building blocks.

pub mod frame_allocator;
pub mod heap;
pub mod paging;

pub use frame_allocator::BootInfoFrameAllocator;
