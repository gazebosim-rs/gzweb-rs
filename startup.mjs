// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
// Select before instantiating either bundle: wgpu30 Instance::new chooses
// navigator.gpu's backend before discovering whether it has an adapter.
export async function selectBackend(navigator, makeCanvas) {
    if (navigator.gpu && typeof navigator.gpu.requestAdapter === 'function') {
        try {
            if (await navigator.gpu.requestAdapter({powerPreference: 'high-performance'})) return 'webgpu';
        } catch (_) { /* Capability discovery, before running the application. */ }
    }
    const canvas = makeCanvas();
    let gl;
    try {
        gl = canvas.getContext('webgl2');
        if (gl) return 'webgl2';
    } finally {
        gl?.getExtension('WEBGL_lose_context')?.loseContext();
    }
    throw new Error('No usable WebGPU adapter or WebGL2 context. Enable browser graphics and reload.');
}
export function failStartup(document, error) {
    document.documentElement.setAttribute('data-viewer-state', 'failed');
    document.getElementById('bevy-status').textContent = `Viewer stopped: ${error}`;
    document.getElementById('bevy-canvas').hidden = true;
}
