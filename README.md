# Tap is a blazing fast, GPUI powered note taking app. 

### Erm... why?
This is a [Cap](https://cap.so/) team internal hackathon project. We wanted to experiment with GPUI, so this is what I chose to build.

### How is it fast?

[**GPUI**](https://www.gpui.rs/) is a minimalist Rust UI layer that talks straight to `wgpu`, so every pixel is drawn by your graphics card instead of the CPU.

* 💨 Micro-latency redraws — the GPU’s thousands of cores handle text layout and rendering in parallel.  
* 🔋 Lower battery / CPU burn
* 🪶 Tiny footprint — a native Rust binary (a few MB) instead of a 200 MB Electron bundle.

No hidden browser, no heavyweight framework—just Rust, `wgpu`, and your GPU doing what it does best.

Imagine this text editor as more of a game instead of an app.
