import type { AppState } from '../types/shader.js';

export interface Uniforms {
  iTime: number;
  iTimeDelta: number;
  iResolution: [number, number, number];
  iMouse: [number, number, number, number];
  iFrame: number;
  iDate: [number, number, number, number];
}

let lastTime = 0;

export function computeUniforms(state: AppState, canvas: HTMLCanvasElement): Uniforms {
  const now = performance.now() / 1000;
  const time = state.isPlaying ? now - state.startTime : state.pausedTime;
  const timeDelta = time - lastTime;
  lastTime = time;
  const d = new Date();
  return {
    iTime: time,
    iTimeDelta: Math.max(0, timeDelta),
    iResolution: [canvas.width, canvas.height, canvas.width / canvas.height],
    iMouse: [
      state.mouse.x,
      state.mouse.y,
      state.mouse.down ? state.mouse.clickX : -state.mouse.clickX,
      state.mouse.down ? state.mouse.clickY : -state.mouse.clickY,
    ],
    iFrame: state.frameCount,
    iDate: [
      d.getFullYear(),
      d.getMonth(),
      d.getDate(),
      d.getHours() * 3600 + d.getMinutes() * 60 + d.getSeconds() + d.getMilliseconds() / 1000,
    ],
  };
}
