import sounddevice as sd
import numpy as np
import time

# Remote configuration parameters
HIGH_PASS_CUTOFF = 80.0
GAIN_MODIFIER = 1.2
SAMPLE_RATE = 44100

def vectorised_high_pass_filter(audio_samples, sample_rate, cutoff):
    """Vectorised filter using fast NumPy array math."""
    rc = 1.0 / (2.0 * np.pi * cutoff)
    dt = 1.0 / sample_rate
    alpha = rc / (rc + dt)
    
    diff = np.diff(audio_samples, prepend=audio_samples[0])
    filtered_samples = alpha * diff
    return filtered_samples * GAIN_MODIFIER

print("[*] Pre-rendering clean audio array to eliminate loop processing delays...")
# Pre-generate 5 full seconds of a completely continuous 440Hz test tone
total_samples = SAMPLE_RATE * 5
timeline = np.arange(total_samples) / SAMPLE_RATE
raw_tone = 0.2 * np.sin(2 * np.pi * 440.0 * timeline)

# Pre-apply our proprietary native DSP filter so no math runs inside the hardware loop
PRE_BUFFERED_AUDIO = vectorised_high_pass_filter(raw_tone, SAMPLE_RATE, HIGH_PASS_CUTOFF)
np.clip(PRE_BUFFERED_AUDIO, -1.0, 1.0, out=PRE_BUFFERED_AUDIO)

# Global playback tracking index pointer
playback_index = 0

def audio_callback(outdata, frames, time_info, status):
    global playback_index
    if status:
        print(f"[!] Hardware Status Alert: {status}", flush=True)
        
    # Calculate how many samples we need to feed the sound card right now
    remainder = len(PRE_BUFFERED_AUDIO) - playback_index
    
    if remainder >= frames:
        # Pull the data directly out of our pre-rendered memory block instantly
        chunk = PRE_BUFFERED_AUDIO[playback_index:playback_index + frames]
        playback_index += frames
    else:
        # Loop back to the beginning of the memory array if we reach the end
        chunk = np.zeros(frames)
        chunk[:remainder] = PRE_BUFFERED_AUDIO[playback_index:]
        playback_index = frames - remainder
        chunk[remainder:] = PRE_BUFFERED_AUDIO[:playback_index]
        
    # Write the clean data chunk directly to the hardware column
    outdata[:] = chunk.reshape(-1, 1).astype(np.float32)

def start_audio_pipeline():
    print("[*] Launching Zero-Latency Pre-Buffered Subsystem...")
    try:
        # Initialize the hardware interface stream targeting your Realtek sound card
        stream = sd.OutputStream(
            channels=1,
            samplerate=SAMPLE_RATE,
            blocksize=512,  # Set a defined, safe buffer block size
            latency='low',  # Force low-latency driver behavior
            callback=audio_callback,
            dtype='float32'
        )
        
        with stream:
            print("[+] Interceptor stream successfully synced with pre-rendered memory.")
            print("[*] Running for 5 seconds to verify stabilization... (Or press Ctrl+C)")
            time.sleep(5.0)
            
    except KeyboardInterrupt:
        print("\n[*] Halting stream execution loop.")
    except Exception as e:
        print(f"[-] Subsystem exception: {e}")
    finally:
        print("[+] Hardware layers decoupled successfully.")

if __name__ == "__main__":
    start_audio_pipeline()