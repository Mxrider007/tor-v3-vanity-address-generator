//! kernel.cu compiled for the CPU (the `kernel-test` feature), so the tests
//! can check the CUDA kernel's arithmetic and walk without an NVIDIA GPU.

use std::cell::RefCell;

use super::super::{Device, Hit, MAX_HITS, SELF_TEST_TARGETS, parse_hits, table_words_packed};

unsafe extern "C" {
    fn torv3_cuda_search(
        groups: u32,
        threads: u32,
        filter_count: u32,
        filter_words: u32,
        table: *const u32,
        filters: *const u32,
        centers: *mut u32,
        hits: *mut u32,
    );
    fn torv3_cuda_field_op(op: u32, a: *const u32, b: *const u32, out: *mut u32);
}

/// Field operations of the kernel, on packed little-endian words.
#[derive(Debug, Clone, Copy)]
pub(in crate::search::gpu) enum FieldOp {
    Mul = 0,
    Square = 1,
    Invert = 2,
    /// (a + b)·(a − b)
    SumTimesDifference = 3,
}

pub(in crate::search::gpu) fn field_op(op: FieldOp, a: &[u32; 8], b: &[u32; 8]) -> [u32; 8] {
    let mut out = [0u32; 8];
    // SAFETY: the function reads eight words from `a` and `b` and writes
    // eight to `out`.
    unsafe { torv3_cuda_field_op(op as u32, a.as_ptr(), b.as_ptr(), out.as_mut_ptr()) };
    out
}

/// The CUDA kernel running on the CPU, one thread after another.
pub(in crate::search::gpu) struct HostKernel {
    table: Vec<u32>,
    filters: RefCell<Vec<u32>>,
    centers: RefCell<Vec<u32>>,
    max_threads: u32,
}

impl HostKernel {
    pub(in crate::search::gpu) fn new(max_threads: u32, max_filters: usize) -> HostKernel {
        HostKernel {
            table: table_words_packed(),
            filters: RefCell::new(vec![0; max_filters.max(SELF_TEST_TARGETS) * 16]),
            centers: RefCell::new(vec![0; max_threads as usize * 16]),
            max_threads,
        }
    }
}

impl Device for HostKernel {
    fn max_threads(&self) -> u32 {
        self.max_threads
    }

    fn upload_centers(&self, words: &[u32]) -> Result<(), String> {
        self.centers.borrow_mut()[..words.len()].copy_from_slice(words);
        Ok(())
    }

    fn upload_filters(&self, filters: &[[u32; 16]]) -> Result<(), String> {
        let words = filters.as_flattened();
        self.filters.borrow_mut()[..words.len()].copy_from_slice(words);
        Ok(())
    }

    fn dispatch(
        &self,
        groups: u32,
        threads: u32,
        filters: u32,
        words: u32,
    ) -> Result<(Vec<Hit>, u32), String> {
        assert!(
            threads <= self.max_threads && filters as usize * 16 <= self.filters.borrow().len()
        );
        let mut hits = vec![0u32; 1 + 3 * MAX_HITS];
        // SAFETY: the buffers have the sizes the kernel expects for these
        // arguments (checked above).
        unsafe {
            torv3_cuda_search(
                groups,
                threads,
                filters,
                words,
                self.table.as_ptr(),
                self.filters.borrow().as_ptr(),
                self.centers.borrow_mut().as_mut_ptr(),
                hits.as_mut_ptr(),
            );
        }
        Ok(parse_hits(&hits))
    }

    fn busy(&self) -> bool {
        false
    }

    fn read_centers(&self, threads: u32) -> Result<Vec<u32>, String> {
        Ok(self.centers.borrow()[..threads as usize * 16].to_vec())
    }
}
