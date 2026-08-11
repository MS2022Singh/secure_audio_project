import numpy as np
import soundfile as sf
from scipy.signal import lfilter

def embed_invisible_watermark(input_signal, sample_rate, watermark_bitstream):
    """
    Embeds a cryptographic bitstream using Host Interference Cancellation (HIC).
    Vectorized using scipy.signal.lfilter to maintain real-time performance at 192kHz.
    """
    watermarked_signal = np.array(input_signal, copy=True, dtype=np.float64)
    total_samples = len(watermarked_signal)
    num_bits = len(watermark_bitstream)
    
    BLOCK_SIZE = 1024
    total_blocks = total_samples // BLOCK_SIZE
    blocks_per_bit = total_blocks // num_bits
    
    if blocks_per_bit == 0:
        blocks_per_bit = 1
        
    bit_length = blocks_per_bit * BLOCK_SIZE
    watermark_gain = 0.0005  

    # IIR Filter Coefficients for Exponential Smoothing Filter
    # Equation: y[n] = alpha * x[n] + (1 - alpha) * y[n-1]
    alpha = 0.002  
    b = [alpha]
    a = [1.0, -(1.0 - alpha)]
    
    # Track the initial filter conditions across block boundaries seamlessly
    zi = np.array([0.0])

    for i, bit in enumerate(watermark_bitstream):
        start = i * bit_length
        end = start + bit_length
        
        if end > total_samples:
            break
            
        current_chunk_len = end - start

        # 1. Generate raw noise sequence
        np.random.seed(6000 + i)
        raw_noise = np.random.choice([-1.0, 1.0], current_chunk_len)

        # 2. HIGH-SPEED FIX: Vectorized continuous-state IIR filter using C-accelerated lfilter
        noise, zi = lfilter(b, a, raw_noise, zi=zi)
            
        noise_std = np.std(noise)
        if noise_std > 0:
            noise = noise / noise_std

        # 3. Perform HIC math over the seamlessly smooth vector
        host_slice = np.array(input_signal[start:end], copy=True, dtype=np.float64)
        host_correlation = np.sum(host_slice * noise)

        target_projection = watermark_gain * current_chunk_len if bit == 1 else -watermark_gain * current_chunk_len
        
        compensation_signal = ((target_projection - host_correlation) / current_chunk_len) * noise
        watermarked_signal[start:end] = host_slice + compensation_signal

    return watermarked_signal.astype(np.float64)

def extract_invisible_watermark(watermarked_signal, sample_rate, expected_bit_length):
    """
    Extracts the hidden payload using identical high-speed vectorized filter states.
    """
    total_samples = len(watermarked_signal)
    BLOCK_SIZE = 1024
    total_blocks = total_samples // BLOCK_SIZE
    blocks_per_bit = total_blocks // expected_bit_length
    
    if blocks_per_bit == 0:
        blocks_per_bit = 1
        
    bit_length = blocks_per_bit * BLOCK_SIZE
    recovered_bits = []

    alpha = 0.002
    b = [alpha]
    a = [1.0, -(1.0 - alpha)]
    zi = np.array([0.0])

    for i in range(expected_bit_length):
        start = i * bit_length
        end = start + bit_length
        
        if end > total_samples:
            recovered_bits.append(0)
            continue
            
        current_chunk_len = end - start

        np.random.seed(6000 + i)
        raw_noise = np.random.choice([-1.0, 1.0], current_chunk_len)
        
        noise, zi = lfilter(b, a, raw_noise, zi=zi)
        
        noise_std = np.std(noise)
        if noise_std > 0:
            noise = noise / noise_std

        projection = np.sum(watermarked_signal[start:end] * noise)

        if projection > 0:
            recovered_bits.append(1)
        else:
            recovered_bits.append(0)

    return recovered_bits

if __name__ == "__main__":
    print("--- Steganographic Watermarking Verification Environment ---")
    duration = 1.0
    sr = 192000
    t = np.linspace(0, duration, int(sr * duration), endpoint=False)
    mock_user_audio = 0.5 * np.sin(2 * np.pi * 440.0 * t)

    secret_watermark_bits = [1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 1, 1, 0, 0]
    print(f"[*] Signature Payload to Hide: {secret_watermark_bits}")

    protected_audio = embed_invisible_watermark(mock_user_audio, sr, secret_watermark_bits)
    output_filename = "protected_output.wav"
    sf.write(output_filename, protected_audio, sr)
    print(f"[+] Protected file successfully generated and saved to: {output_filename}")

    extracted_audio, test_sr = sf.read(output_filename)
    decoded_bits = extract_invisible_watermark(extracted_audio, test_sr, len(secret_watermark_bits))
    print(f"[+] Recovered Signature Bits:  {decoded_bits}")

    if decoded_bits == secret_watermark_bits:
        print("\n[SUCCESS] Watermark Verification Passed!")
    else:
        print("\n[ERROR] Watermark payload corruption detected.")