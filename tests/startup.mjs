// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
import assert from 'node:assert/strict';
import {selectBackend, failStartup} from '../startup.mjs';
let probed=0, released=0;
const canvas=()=>({getContext(kind){assert.equal(kind,'webgl2');probed++;return {getExtension(){return {loseContext(){released++;}};}};}});
assert.equal(await selectBackend({gpu:{requestAdapter:async()=>({})}},canvas),'webgpu');assert.equal(probed,0);
assert.equal(await selectBackend({gpu:{requestAdapter:async()=>null}},canvas),'webgl2');
assert.equal(await selectBackend({gpu:{requestAdapter:async()=>{throw Error('denied');}}},canvas),'webgl2');
assert.equal(await selectBackend({},canvas),'webgl2');assert.equal(released,3);
await assert.rejects(selectBackend({},()=>({getContext:()=>null})),/No usable/);
const elements={'bevy-status':{},'bevy-canvas':{}};let state;
failStartup({documentElement:{setAttribute(k,v){assert.equal(k,'data-viewer-state');state=v;}},getElementById:id=>elements[id]},'failure');
assert.equal(state,'failed');assert.equal(elements['bevy-canvas'].hidden,true);assert.match(elements['bevy-status'].textContent,/failure/);
