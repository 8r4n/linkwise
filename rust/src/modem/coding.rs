//! VARA HF DATA coding. Protocol reference and fixture provenance are recorded
//! in reference/vara-hf-recordings.md.

pub(super) struct Random24(pub u32);

impl Random24 {
    pub(super) fn advance(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(0x43fd_43fd).wrapping_add(0xc3_9ec3) & 0xff_ffff;
        self.0
    }

    pub(super) fn draw(&mut self, size: usize) -> usize {
        // The wire generator rounds the product to single precision.
        (self.advance() as f32 * (1.0 / 16_777_216.0) * size as f32) as usize
    }

    pub(super) fn permutation(&mut self, size: usize) -> Vec<usize> {
        let mut result = Vec::with_capacity(size);
        let mut seen = vec![false; size];
        while result.len() < size {
            let value = self.draw(size);
            if !std::mem::replace(&mut seen[value], true) {
                result.push(value);
            }
        }
        result
    }
}

pub(super) fn crc(bytes: &[u8]) -> u16 {
    let mut register = 0xffff_u16;
    for &byte in bytes {
        register ^= u16::from(byte) << 8;
        for _ in 0..8 {
            let polynomial = if register & 0x8000 != 0 { 0x1021 } else { 0 };
            register = (register << 1) ^ polynomial;
        }
    }
    !register
}

pub(super) fn whiten(bits: &mut [u8]) {
    let mut random = Random24(29_318);
    for bit in bits {
        *bit ^= (random.advance() >> 23) as u8;
    }
}

fn transition(state: usize, input: usize) -> (usize, usize) {
    let feedback = input ^ ((state >> 1) & 1) ^ (state >> 2);
    let parity = feedback ^ (state & 1) ^ (state >> 2);
    (((state << 1) & 7) | feedback, parity)
}

fn constituent(bits: impl IntoIterator<Item = u8>) -> (Vec<u8>, [[u8; 2]; 3]) {
    let mut state = 0;
    let mut parity = Vec::new();
    for bit in bits {
        let (next, output) = transition(state, usize::from(bit));
        state = next;
        parity.push(output as u8);
    }
    let tail = std::array::from_fn(|_| {
        let input = ((state >> 1) ^ (state >> 2)) & 1;
        let (next, output) = transition(state, input);
        state = next;
        [input as u8, output as u8]
    });
    (parity, tail)
}

pub(super) fn encode(bits: &[u8], interleaver: &[usize], rate_third: bool) -> Vec<u8> {
    let (first, first_tail) = constituent(bits.iter().copied());
    let (second, second_tail) = constituent(interleaver.iter().map(|&i| bits[i]));
    let mut output = Vec::new();
    for (i, &bit) in bits.iter().enumerate() {
        output.push(bit);
        if rate_third {
            output.extend([first[i], second[i]]);
        } else {
            output.push(if i % 2 == 0 { first[i] } else { second[i] });
        }
    }
    for (first, second) in first_tail.into_iter().zip(second_tail) {
        output.extend(first);
        output.extend(second);
    }
    output
}

// Max-log-MAP on the terminated eight-state trellis. Positive metrics favor 1.
fn extrinsic(symbols: &[[f64; 2]], prior: &[f64]) -> Vec<f64> {
    let mut forward = vec![[f64::NEG_INFINITY; 8]; symbols.len() + 1];
    forward[0][0] = 0.0;
    let branch = |i: usize, input: usize, parity: usize| {
        (symbols[i][0] + prior.get(i).copied().unwrap_or(0.0)) * input as f64
            + symbols[i][1] * parity as f64
    };
    for i in 0..symbols.len() {
        for state in 0..8 {
            for input in 0..2 {
                let (next, parity) = transition(state, input);
                let metric = forward[i][state] + branch(i, input, parity);
                forward[i + 1][next] = forward[i + 1][next].max(metric);
            }
        }
    }

    let mut output = vec![0.0; prior.len()];
    let mut backward = [f64::NEG_INFINITY; 8];
    backward[0] = 0.0;
    for i in (0..symbols.len()).rev() {
        let mut previous = [f64::NEG_INFINITY; 8];
        let mut best = [f64::NEG_INFINITY; 2];
        for (state, metric) in previous.iter_mut().enumerate() {
            for (input, best_input) in best.iter_mut().enumerate() {
                let (next, parity) = transition(state, input);
                let path = backward[next] + branch(i, input, parity);
                *metric = metric.max(path);
                *best_input = best_input.max(forward[i][state] + path);
            }
        }
        backward = previous;
        if i < output.len() {
            output[i] = best[1] - best[0] - symbols[i][0] - prior[i];
        }
    }
    output
}

pub(super) fn decode<T>(
    coded: &[f64],
    interleaver: &[usize],
    rate_third: bool,
    check: impl Fn(&[u8]) -> Option<T>,
) -> Option<T> {
    let size = interleaver.len();
    let stride = if rate_third { 3 } else { 2 };
    let hard_bits: Vec<_> = coded
        .chunks_exact(stride)
        .take(size)
        .map(|bits| u8::from(bits[0] > 0.0))
        .collect();
    if let Some(frame) = check(&hard_bits) {
        return Some(frame);
    }

    let mut first = Vec::with_capacity(size + 3);
    let mut second = Vec::with_capacity(size + 3);
    for (i, &source) in interleaver.iter().enumerate() {
        let (p1, p2) = if rate_third {
            (coded[stride * i + 1], coded[stride * i + 2])
        } else if i % 2 == 0 {
            (coded[stride * i + 1], 0.0)
        } else {
            (0.0, coded[stride * i + 1])
        };
        first.push([coded[stride * i], p1]);
        second.push([coded[stride * source], p2]);
    }
    for tail in coded[stride * size..].as_chunks::<4>().0 {
        first.push([tail[0], tail[1]]);
        second.push([tail[2], tail[3]]);
    }

    let mut prior = vec![0.0; size];
    let mut bits = hard_bits;
    for _ in 0..8 {
        let first_output = extrinsic(&first, &prior);
        let second_prior: Vec<_> = interleaver.iter().map(|&i| first_output[i]).collect();
        let second_output = extrinsic(&second, &second_prior);
        for (i, &source) in interleaver.iter().enumerate() {
            prior[source] = second_output[i];
            bits[source] = u8::from(first[source][0] + first_output[source] + prior[source] > 0.0);
        }
        if let Some(frame) = check(&bits) {
            return Some(frame);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_genibus_check_value() {
        assert_eq!(crc(b"123456789"), 0xd64e);
    }

    #[test]
    fn parity_repairs_corrupted_systematic_bits_at_both_rates() {
        let bits: Vec<_> = (0..96_u16)
            .map(|i| (((i * 7) ^ (i >> 2)) & 1) as u8)
            .collect();
        let interleaver = Random24(12_600_710).permutation(bits.len());
        for rate_third in [false, true] {
            let mut received: Vec<_> = encode(&bits, &interleaver, rate_third)
                .into_iter()
                .map(|bit| f64::from(bit) * 2.0 - 1.0)
                .collect();
            let stride = if rate_third { 3 } else { 2 };
            for i in [0, 14, 39, 71, 90] {
                received[i * stride] *= -0.25;
            }
            assert_eq!(
                decode(&received, &interleaver, rate_third, |result| {
                    (result == bits).then_some(())
                }),
                Some(())
            );
        }
    }
}
