import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { initCore } from './lib/nbp';

// The Rust core (WASM) parses and encodes NBP for both builds: load it first.
await initCore();

export default mount(App, { target: document.getElementById('app')! });
