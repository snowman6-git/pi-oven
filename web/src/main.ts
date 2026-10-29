import '@fontsource-variable/jetbrains-mono';
import './app.css';
import './lib/maple';
import { mount } from 'svelte';
import { settings } from './lib/settings.svelte';
import App from './App.svelte';

settings.apply();

export default mount(App, { target: document.getElementById('app')! });
