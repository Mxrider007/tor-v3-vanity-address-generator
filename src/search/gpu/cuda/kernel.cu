// torv3-vanity CUDA kernel: the same search as ../kernel.wgsl (read that
// file for how the walk works), for NVIDIA GPUs.
//
// Field elements of GF(2^255 - 19) use the representation of the ref10
// reference code: ten signed limbs of alternately 26 and 25 bits, multiplied
// with 64-bit products. Bounds, as in ref10:
//   * "carried": |limb| <= 1.1 * 2^25 (even limbs) or 1.1 * 2^24 (odd limbs).
//     fe_mul, fe_sq and fe_unpack return carried elements.
//   * fe_add / fe_sub of two carried elements may be passed to fe_mul and
//     fe_sq (which accept |limb| <= 1.65 * 2^26 / 1.65 * 2^25), but not to
//     fe_add, fe_sub or fe_canonical.
//
// kernel.ptx is generated from this file by scripts/build-cuda-kernel.sh,
// with clang and without the CUDA toolkit; the NVIDIA driver compiles the PTX
// for the GPU at run time. With the `kernel-test` feature the same file is
// built as ordinary C++, so the tests can run the kernel on the CPU.

#define GROUP 64
#define KEYS_PER_GROUP (2 * GROUP + 1)
#define MAX_HITS 1024

typedef unsigned int u32;
typedef int i32;
typedef long long i64;
typedef unsigned long long u64;

#ifdef __CUDA_ARCH__
#define DEVICE static __attribute__((device)) inline
#define UNROLL _Pragma("unroll")
#define ATOMIC_ADD(p, v) __atomic_fetch_add((p), (v), __ATOMIC_RELAXED)
#else
// The tests run one thread at a time.
#define DEVICE static inline
#define UNROLL
#define ATOMIC_ADD(p, v) ((*(p) += (v)) - (v))
#endif

typedef struct {
    i32 v[10];
} fe;

// Bit width and position of limb i.
#define WIDTH(i) (((i) & 1) ? 25 : 26)
#define POS(i) (26 * (((i) + 1) / 2) + 25 * ((i) / 2))

DEVICE fe fe_one() {
    fe r;
    UNROLL
    for (int i = 0; i < 10; i++) {
        r.v[i] = 0;
    }
    r.v[0] = 1;
    return r;
}

DEVICE fe fe_add(fe f, fe g) {
    fe r;
    UNROLL
    for (int i = 0; i < 10; i++) {
        r.v[i] = f.v[i] + g.v[i];
    }
    return r;
}

DEVICE fe fe_sub(fe f, fe g) {
    fe r;
    UNROLL
    for (int i = 0; i < 10; i++) {
        r.v[i] = f.v[i] - g.v[i];
    }
    return r;
}

// Round limb i (below 9) to a signed carry into limb i + 1.
DEVICE void carry(i64 h[10], int i) {
    i64 c = (h[i] + ((i64)1 << (WIDTH(i) - 1))) >> WIDTH(i);
    h[i + 1] += c;
    h[i] -= c * ((i64)1 << WIDTH(i));
}

// Carry 64-bit column sums into a carried element (ref10's carry order).
DEVICE fe fe_reduce(i64 h[10]) {
    carry(h, 0);
    carry(h, 4);
    carry(h, 1);
    carry(h, 5);
    carry(h, 2);
    carry(h, 6);
    carry(h, 3);
    carry(h, 7);
    carry(h, 4);
    carry(h, 8);
    i64 c = (h[9] + ((i64)1 << 24)) >> 25;
    h[0] += c * 19;
    h[9] -= c * ((i64)1 << 25);
    carry(h, 0);
    fe r;
    UNROLL
    for (int i = 0; i < 10; i++) {
        r.v[i] = (i32)h[i];
    }
    return r;
}

// Limb i times limb j has weight 2^(POS(i) + POS(j)): twice the weight of
// column i + j when both are odd, and 2^255 = 19 times the weight of column
// i + j - 10 when i + j >= 10.
DEVICE fe fe_mul(fe f, fe g) {
    i64 h[10];
    UNROLL
    for (int k = 0; k < 10; k++) {
        h[k] = 0;
    }
    UNROLL
    for (int i = 0; i < 10; i++) {
        UNROLL
        for (int j = 0; j < 10; j++) {
            i32 a = (i & j & 1) ? 2 * f.v[i] : f.v[i];
            i32 b = (i + j >= 10) ? 19 * g.v[j] : g.v[j];
            h[(i + j) % 10] += (i64)a * b;
        }
    }
    return fe_reduce(h);
}

DEVICE fe fe_sq(fe f) {
    i64 h[10];
    UNROLL
    for (int k = 0; k < 10; k++) {
        h[k] = 0;
    }
    UNROLL
    for (int i = 0; i < 10; i++) {
        UNROLL
        for (int j = i; j < 10; j++) {
            i32 a = (i == j) ? f.v[i] : 2 * f.v[i];
            i32 b = f.v[j];
            if (i & j & 1) {
                b *= 2;
            }
            if (i + j >= 10) {
                b *= 19;
            }
            h[(i + j) % 10] += (i64)a * b;
        }
    }
    return fe_reduce(h);
}

DEVICE fe fe_sqn(fe f, int n) {
    for (int i = 0; i < n; i++) {
        f = fe_sq(f);
    }
    return f;
}

// z^(p - 2), the same addition chain as the CPU code and kernel.wgsl.
DEVICE fe fe_inv(fe z) {
    fe z2 = fe_sq(z);
    fe z9 = fe_mul(fe_sqn(z2, 2), z);
    fe z11 = fe_mul(z9, z2);
    fe z2_5_0 = fe_mul(fe_sq(z11), z9);
    fe z2_10_0 = fe_mul(fe_sqn(z2_5_0, 5), z2_5_0);
    fe z2_20_0 = fe_mul(fe_sqn(z2_10_0, 10), z2_10_0);
    fe z2_40_0 = fe_mul(fe_sqn(z2_20_0, 20), z2_20_0);
    fe z2_50_0 = fe_mul(fe_sqn(z2_40_0, 10), z2_10_0);
    fe z2_100_0 = fe_mul(fe_sqn(z2_50_0, 50), z2_50_0);
    fe z2_200_0 = fe_mul(fe_sqn(z2_100_0, 100), z2_100_0);
    fe z2_250_0 = fe_mul(fe_sqn(z2_200_0, 50), z2_50_0);
    return fe_mul(fe_sqn(z2_250_0, 5), z11);
}

// A field element from eight little-endian 32-bit words (bit 255 ignored).
DEVICE fe fe_unpack(const u32 *w) {
    i64 h[10];
    UNROLL
    for (int i = 0; i < 10; i++) {
        u64 window = w[POS(i) / 32];
        if (POS(i) / 32 + 1 < 8) {
            window |= (u64)w[POS(i) / 32 + 1] << 32;
        }
        h[i] = (i64)((window >> (POS(i) % 32)) & ((1u << WIDTH(i)) - 1));
    }
    return fe_reduce(h);
}

// The fully reduced value (0 <= v < p) as eight little-endian 32-bit words
// (ref10's fe_tobytes). `f` must be carried.
DEVICE void fe_canonical(fe f, u32 out[8]) {
    i32 h[10];
    UNROLL
    for (int i = 0; i < 10; i++) {
        h[i] = f.v[i];
    }
    // q = 1 if the value is at least p, else 0.
    i32 q = (19 * h[9] + ((i32)1 << 24)) >> 25;
    UNROLL
    for (int i = 0; i < 10; i++) {
        q = (h[i] + q) >> WIDTH(i);
    }
    h[0] += 19 * q;
    UNROLL
    for (int i = 0; i < 9; i++) {
        i32 c = h[i] >> WIDTH(i);
        h[i + 1] += c;
        h[i] -= c * ((i32)1 << WIDTH(i));
    }
    h[9] &= ((i32)1 << 25) - 1;

    u64 acc = 0;
    int bits = 0;
    int word = 0;
    UNROLL
    for (int i = 0; i < 10; i++) {
        acc |= (u64)(u32)h[i] << bits;
        bits += WIDTH(i);
        if (bits >= 32) {
            out[word++] = (u32)acc;
            acc >>= 32;
            bits -= 32;
        }
    }
    out[7] = (u32)acc;
}

// (GROUP + 1) entries of x, y and e = d·x·y as packed words: i·8B for
// i = 1..=GROUP, then the step (2·GROUP + 1)·8B to the next center.
DEVICE fe table_entry(const u32 *table, u32 i, u32 which) {
    return fe_unpack(table + (i * 3 + which) * 8);
}

struct Params {
    u32 groups;
    u32 filter_count;
    u32 filter_words;
    // Per filter: 8 mask words then 8 value words over the packed y.
    const u32 *filters;
    // hits[0] = number of hits; then (thread, group, index) triples.
    u32 *hits;
};

DEVICE bool is_candidate(const u32 y[8], const Params *p) {
    for (u32 f = 0; f < p->filter_count; f++) {
        bool ok = true;
        for (u32 w = 0; w < p->filter_words; w++) {
            if ((y[w] & p->filters[f * 16 + w]) != p->filters[f * 16 + 8 + w]) {
                ok = false;
                break;
            }
        }
        if (ok) {
            return true;
        }
    }
    return false;
}

DEVICE void report(fe y, u32 thread, u32 group, u32 index, const Params *p) {
    u32 packed[8];
    fe_canonical(y, packed);
    if (is_candidate(packed, p)) {
        u32 slot = ATOMIC_ADD(&p->hits[0], 1u);
        if (slot < MAX_HITS) {
            p->hits[1 + 3 * slot] = thread;
            p->hits[2 + 3 * slot] = group;
            p->hits[3 + 3 * slot] = index;
        }
    }
}

// One thread: walk `groups` groups from its center (packed x then packed y
// in centers[16 t ..]) and store the next center back.
DEVICE void search_thread(u32 t, const Params *p, const u32 *table, u32 *centers) {
    fe x1 = fe_unpack(centers + t * 16);
    fe y1 = fe_unpack(centers + t * 16 + 8);
    fe t1 = fe_mul(x1, y1);
    fe one = fe_one();
    // Prefix products of the denominators.
    fe pre[2 * GROUP + 2];

    for (u32 g = 0; g < p->groups; g++) {
        // Forward pass: denominators 1 - k and 1 + k with k = t1·e.
        fe acc = one;
        for (u32 i = 0; i < GROUP; i++) {
            fe k = fe_mul(t1, table_entry(table, i, 2));
            pre[2 * i] = acc;
            acc = fe_mul(acc, fe_sub(one, k));
            pre[2 * i + 1] = acc;
            acc = fe_mul(acc, fe_add(one, k));
        }
        fe kr = fe_mul(t1, table_entry(table, GROUP, 2));
        fe dp_r = fe_sub(one, kr);
        fe dm_r = fe_add(one, kr);
        pre[2 * GROUP] = acc;
        acc = fe_mul(acc, dp_r);
        pre[2 * GROUP + 1] = acc;
        acc = fe_mul(acc, dm_r);

        // Backward pass: peel off one inverse at a time.
        fe inv = fe_inv(acc);
        fe inv_dm_r = fe_mul(inv, pre[2 * GROUP + 1]);
        inv = fe_mul(inv, dm_r);
        fe inv_dp_r = fe_mul(inv, pre[2 * GROUP]);
        inv = fe_mul(inv, dp_r);
        for (u32 j = 0; j < GROUP; j++) {
            u32 i = GROUP - 1 - j;
            fe xx = fe_mul(x1, table_entry(table, i, 0));
            fe yy = fe_mul(y1, table_entry(table, i, 1));
            fe k = fe_mul(t1, table_entry(table, i, 2));
            fe inv_dm = fe_mul(inv, pre[2 * i + 1]);
            inv = fe_mul(inv, fe_add(one, k));
            fe inv_dp = fe_mul(inv, pre[2 * i]);
            inv = fe_mul(inv, fe_sub(one, k));
            // y(P + Q_i) = (yy + xx) / (1 - k),  y(P - Q_i) = (yy - xx) / (1 + k)
            report(fe_mul(fe_add(yy, xx), inv_dp), t, g, 2 * i + 1, p);
            report(fe_mul(fe_sub(yy, xx), inv_dm), t, g, 2 * i + 2, p);
        }
        report(y1, t, g, 0, p);

        // Next center: P + R.
        fe rx = table_entry(table, GROUP, 0);
        fe ry = table_entry(table, GROUP, 1);
        fe ny = fe_mul(fe_add(fe_mul(y1, ry), fe_mul(x1, rx)), inv_dp_r);
        fe nx = fe_mul(fe_add(fe_mul(x1, ry), fe_mul(y1, rx)), inv_dm_r);
        x1 = nx;
        y1 = ny;
        t1 = fe_mul(x1, y1);
    }

    fe_canonical(x1, centers + t * 16);
    fe_canonical(y1, centers + t * 16 + 8);
}

#ifdef __CUDA_ARCH__

extern "C" __attribute__((global)) void search(
    u32 groups,
    u32 threads,
    u32 filter_count,
    u32 filter_words,
    const u32 *__restrict__ table,
    const u32 *__restrict__ filters,
    u32 *__restrict__ centers,
    u32 *__restrict__ hits) {
    u32 t = __nvvm_read_ptx_sreg_ctaid_x() * __nvvm_read_ptx_sreg_ntid_x() +
            __nvvm_read_ptx_sreg_tid_x();
    if (t >= threads) {
        return;
    }
    Params p = {groups, filter_count, filter_words, filters, hits};
    search_thread(t, &p, table, centers);
}

#else

// Entry points for the tests (src/search/gpu/cuda/host.rs).

extern "C" void torv3_cuda_search(
    u32 groups,
    u32 threads,
    u32 filter_count,
    u32 filter_words,
    const u32 *table,
    const u32 *filters,
    u32 *centers,
    u32 *hits) {
    Params p = {groups, filter_count, filter_words, filters, hits};
    for (u32 t = 0; t < threads; t++) {
        search_thread(t, &p, table, centers);
    }
}

// op: 0 = a·b, 1 = a², 2 = 1/a, 3 = (a + b)·(a - b) with carried inputs.
extern "C" void torv3_cuda_field_op(u32 op, const u32 *a, const u32 *b, u32 *out) {
    fe x = fe_unpack(a);
    fe y = fe_unpack(b);
    fe r;
    switch (op) {
    case 0:
        r = fe_mul(x, y);
        break;
    case 1:
        r = fe_sq(x);
        break;
    case 2:
        r = fe_inv(x);
        break;
    default:
        r = fe_mul(fe_add(x, y), fe_sub(x, y));
        break;
    }
    fe_canonical(r, out);
}

#endif
