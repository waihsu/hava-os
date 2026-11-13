//! Access to the active x86_64 page tables.

use x86_64::{
    VirtAddr,
    structures::paging::{
        FrameAllocator, Mapper, Page, PageTable, PageTableFlags, RecursivePageTable, Size4KiB,
    },
};

pub const EXAMPLE_PAGE_ADDRESS: u64 = 0x_4444_4444_0000;
pub const EXAMPLE_VALUE: u64 = 0x_4e4f_5641_2026;

/// Creates a mapper for the page tables currently active in the CPU.
///
/// The bootloader creates the recursive mapping and supplies its level-4 index
/// through `BootInfo`. This function is unsafe because there must be only one
/// mutable accessor to the active page tables at a time.
pub unsafe fn init(recursive_index: u16) -> RecursivePageTable<'static> {
    let index = u64::from(recursive_index);
    let address = (index << 39) | (index << 30) | (index << 21) | (index << 12);
    let table_address = VirtAddr::new_truncate(address).as_u64() as *mut PageTable;
    let level_4_table = unsafe { &mut *table_address };

    RecursivePageTable::new(level_4_table)
        .expect("bootloader recursive mapping must reference the active level-4 table")
}

/// Creates one writable 4 KiB mapping and verifies that it can store data.
///
/// The selected virtual page is reserved for this learning demonstration. The
/// allocator must return frames that are not already in use.
pub unsafe fn map_example_page(
    mapper: &mut RecursivePageTable,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> (VirtAddr, u64) {
    let page = Page::containing_address(VirtAddr::new(EXAMPLE_PAGE_ADDRESS));
    let frame = frame_allocator
        .allocate_frame()
        .expect("out of usable physical frames");
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    unsafe {
        mapper
            .map_to(page, frame, flags, frame_allocator)
            .expect("example virtual page must be unused")
            .flush();
    }

    let pointer = page.start_address().as_u64() as *mut u64;
    unsafe {
        pointer.write_volatile(EXAMPLE_VALUE);
        (page.start_address(), pointer.read_volatile())
    }
}
