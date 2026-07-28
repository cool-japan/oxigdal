// Fast Fourier Transform (FFT) on GPU using Cooley-Tukey algorithm
// Implements both forward and inverse FFT with bit-reversal permutation

struct Complex {
    real: f32,
    imag: f32,
}

@group(0) @binding(0) var<storage, read_write> data: array<Complex>;
@group(0) @binding(1) var<uniform> params: FftParams;

struct FftParams {
    n: u32,           // Size of FFT (must be power of 2)
    inverse: u32,     // 1 for inverse FFT, 0 for forward
    stage: u32,       // Current FFT stage
    pad: u32,
}

const PI: f32 = 3.14159265359;

// Complex multiplication
fn complex_mul(a: Complex, b: Complex) -> Complex {
    return Complex(
        a.real * b.real - a.imag * b.imag,
        a.real * b.imag + a.imag * b.real,
    );
}

// Complex addition
fn complex_add(a: Complex, b: Complex) -> Complex {
    return Complex(a.real + b.real, a.imag + b.imag);
}

// Complex subtraction
fn complex_sub(a: Complex, b: Complex) -> Complex {
    return Complex(a.real - b.real, a.imag - b.imag);
}

// Twiddle factor (roots of unity)
fn twiddle_factor(k: u32, n: u32, inverse: bool) -> Complex {
    let angle = -2.0 * PI * f32(k) / f32(n);
    let sign = select(1.0, -1.0, inverse);
    return Complex(cos(angle), sign * sin(angle));
}

// Bit reversal for FFT input permutation
fn bit_reverse(x: u32, bits: u32) -> u32 {
    var result: u32 = 0u;
    var val = x;
    for (var i = 0u; i < bits; i = i + 1u) {
        result = (result << 1u) | (val & 1u);
        val = val >> 1u;
    }
    return result;
}

// Bit-reversal permutation stage
@compute @workgroup_size(256, 1, 1)
fn fft_bit_reverse(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let i = global_id.x;
    if (i >= params.n) {
        return;
    }

    // Calculate number of bits
    var bits = 0u;
    var n = params.n;
    while (n > 1u) {
        n = n >> 1u;
        bits = bits + 1u;
    }

    let rev_i = bit_reverse(i, bits);

    // Only swap if i < rev_i to avoid double swapping
    if (i < rev_i) {
        let temp = data[i];
        data[i] = data[rev_i];
        data[rev_i] = temp;
    }
}

// Single FFT butterfly stage
@compute @workgroup_size(256, 1, 1)
fn fft_butterfly_stage(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let i = global_id.x;
    if (i >= params.n / 2u) {
        return;
    }

    let stage = params.stage;
    let m = 1u << stage;           // 2^stage
    let m2 = m << 1u;              // 2^(stage+1)

    let k = i / m;
    let j = i % m;

    let idx1 = k * m2 + j;
    let idx2 = idx1 + m;

    if (idx2 >= params.n) {
        return;
    }

    let inverse = params.inverse != 0u;
    let w = twiddle_factor(j, m2, inverse);

    let t = complex_mul(w, data[idx2]);
    let u = data[idx1];

    data[idx1] = complex_add(u, t);
    data[idx2] = complex_sub(u, t);
}

// Complete FFT (Cooley-Tukey algorithm)
@compute @workgroup_size(256, 1, 1)
fn fft_cooley_tukey(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let i = global_id.x;
    if (i >= params.n) {
        return;
    }

    // This kernel processes one FFT stage at a time
    // Multiple dispatches are needed for complete FFT
    let stage = params.stage;
    let m = 1u << stage;
    let m2 = m << 1u;

    let group = i / m2;
    let offset = i % m;
    let pair = group * m2 + offset;

    if (pair + m >= params.n) {
        return;
    }

    let inverse = params.inverse != 0u;
    let k = offset;
    let w = twiddle_factor(k, m2, inverse);

    let idx_even = pair;
    let idx_odd = pair + m;

    let t = complex_mul(w, data[idx_odd]);
    let u = data[idx_even];

    data[idx_even] = complex_add(u, t);
    data[idx_odd] = complex_sub(u, t);
}

// Normalization for inverse FFT
@compute @workgroup_size(256, 1, 1)
fn fft_normalize(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let i = global_id.x;
    if (i >= params.n) {
        return;
    }

    let scale = 1.0 / f32(params.n);
    data[i].real = data[i].real * scale;
    data[i].imag = data[i].imag * scale;
}

// In-place iterative Cooley-Tukey FFT over a strided 1-D sequence embedded in
// the `data` buffer: element k of the sequence lives at `data[base + k*stride]`.
// `n` must be a power of two. This is the same radix-2 DIT algorithm as
// `fft_cooley_tukey`, run to completion by a single invocation so that each row
// / column of a 2-D transform is processed independently without barriers.
fn fft_1d_strided(base: u32, stride: u32, n: u32, inverse: bool) {
    // Number of bits needed to index the sequence.
    var bits = 0u;
    var t = n;
    while (t > 1u) {
        t = t >> 1u;
        bits = bits + 1u;
    }

    // Bit-reversal permutation.
    for (var i = 0u; i < n; i = i + 1u) {
        let r = bit_reverse(i, bits);
        if (i < r) {
            let ia = base + i * stride;
            let ib = base + r * stride;
            let tmp = data[ia];
            data[ia] = data[ib];
            data[ib] = tmp;
        }
    }

    // Butterfly stages: sub-transform length grows 2, 4, ..., n.
    var len = 2u;
    while (len <= n) {
        let half = len >> 1u;
        var start = 0u;
        while (start < n) {
            for (var k = 0u; k < half; k = k + 1u) {
                let w = twiddle_factor(k, len, inverse);
                let even_idx = base + (start + k) * stride;
                let odd_idx = base + (start + k + half) * stride;
                let tw = complex_mul(w, data[odd_idx]);
                let u = data[even_idx];
                data[even_idx] = complex_add(u, tw);
                data[odd_idx] = complex_sub(u, tw);
            }
            start = start + len;
        }
        len = len << 1u;
    }
}

// 2D FFT (row-wise pass): one invocation per row performs a complete 1-D FFT
// along that row. Dispatch with `ceil(n / 64)` workgroups in x.
@compute @workgroup_size(64, 1, 1)
fn fft_2d_rows(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let row = global_id.x;
    if (row >= params.n) {
        return;
    }
    fft_1d_strided(row * params.n, 1u, params.n, params.inverse != 0u);
}

// 2D FFT (column-wise pass): one invocation per column performs a complete 1-D
// FFT down that column (stride = n). Run after `fft_2d_rows` to complete a
// separable 2-D transform. Dispatch with `ceil(n / 64)` workgroups in x.
@compute @workgroup_size(64, 1, 1)
fn fft_2d_cols(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let col = global_id.x;
    if (col >= params.n) {
        return;
    }
    fft_1d_strided(col, params.n, params.n, params.inverse != 0u);
}
