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

// 2D FFT (row-wise pass)
@compute @workgroup_size(16, 16, 1)
fn fft_2d_rows(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let row = global_id.y;
    let col = global_id.x;

    if (row >= params.n || col >= params.n) {
        return;
    }

    // Process each row independently
    // This would need to be called multiple times for complete 2D FFT
    let idx = row * params.n + col;

    // Placeholder for row FFT processing
    // In practice, this would call the 1D FFT algorithm
}

// 2D FFT (column-wise pass)
@compute @workgroup_size(16, 16, 1)
fn fft_2d_cols(
    @builtin(global_invocation_id) global_id: vec3<u32>,
) {
    let row = global_id.y;
    let col = global_id.x;

    if (row >= params.n || col >= params.n) {
        return;
    }

    // Process each column independently
    let idx = row * params.n + col;

    // Placeholder for column FFT processing
}
