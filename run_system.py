import subprocess
import sys
import time
import os
import numpy as np
import soundfile as sf

def ensure_test_file_exists(filename="sample.wav"):
    """Generates a clean 5-second sample audio file if the user hasn't dropped one in yet."""
    if not os.path.exists(filename):
        print(f"[*] Missing target asset. Auto-generating test track: {filename}...")
        sample_rate = 44100
        duration = 5.0
        t = np.linspace(0, duration, int(sample_rate * duration), endpoint=False)
        tone = 0.3 * np.sin(2 * np.pi * 440.0 * t) + 0.1 * np.sin(2 * np.pi * 880.0 * t)
        sf.write(filename, tone, sample_rate)
        print(f"[+] Test track {filename} compiled successfully.")

def launch_all():
    print("[*] Launching Secure Audio Engine Infrastructure Pipeline...")
    
    # Ensure the workspace environment has a valid audio file to ingest
    ensure_test_file_exists("sample.wav")
    
    print("[*] Spinning up Cloud Backend API Node...")
    # FIX: Call uvicorn explicitly from module space and pipe logs to stdout/stderr
    server_process = subprocess.Popen(
        [sys.executable, "-m", "uvicorn", "server:app", "--host", "127.0.0.1", "--port", "9000"],
        cwd="backend",
        stdout=sys.stdout,
        stderr=sys.stderr
    )
    
    # Wait for the backend database socket binding to complete
    time.sleep(2.5)
    
    try:
        print("[*] Triggering Desktop Client Security Verification...")
        print("-" * 30)
        
        client_process = subprocess.Popen(
            [sys.executable, "main.py", "--input", "sample.wav", "--output", "protected_output.wav"],
            stdout=sys.stdout,
            stderr=sys.stderr
        )
        client_process.wait()
        print("-" * 30)
        
    except KeyboardInterrupt:
        print("\n[*] Execution interrupted by user key binding.")
    finally:
        print("[*] Cleaning up active framework tasks...")
        server_process.terminate()
        server_process.wait()
        print("[+] System offline safely.")

if __name__ == "__main__":
    launch_all()