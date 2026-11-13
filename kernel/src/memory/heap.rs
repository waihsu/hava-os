//! Dynamic heap setup on top of mapped virtual pages.

use linked_list_allocator::LockedHeap;
use x86_64::{
    VirtAddr,
    structures::paging::{
        FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
    },
};

pub const HEAP_START: usize = 0x_5555_5555_0000;
pub const HEAP_SIZE: usize = 100 * 1024;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Maps a 100 KiB virtual range and hands it to the kernel's global allocator.
pub fn init(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), &'static str> {
    let heap_start = VirtAddr::new(HEAP_START as u64);
    let heap_end = heap_start + HEAP_SIZE as u64 - 1_u64;
    let start_page = Page::containing_address(heap_start);
    let end_page = Page::containing_address(heap_end);
    let page_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    for page in Page::range_inclusive(start_page, end_page) {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or("out of usable physical frames while mapping heap")?;
        unsafe {
            mapper
                .map_to(page, frame, page_flags, frame_allocator)
                .map_err(|_| "heap virtual page was already mapped")?
                .flush();
        }
    }

    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE);
    }
    Ok(())
}
