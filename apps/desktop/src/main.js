import './app.css';
import App from './App.svelte';
import { mount } from 'svelte';

const root_element = document.getElementById('app');
if (!root_element) {
  const error_element = document.createElement('pre');
  error_element.style.cssText = 'padding:24px;color:red';
  error_element.textContent = 'Error: #app element not found';
  document.body.appendChild(error_element);
} else {
  try {
    mount(App, { target: root_element });
  } catch (err) {
    const error_element = document.createElement('pre');
    error_element.style.cssText = 'padding:24px;color:red;white-space:pre-wrap';
    error_element.textContent = `Mount error: ${err instanceof Error ? err.message : String(err)}`;
    root_element.replaceChildren(error_element);
    console.error('Svelte mount failed:', err);
  }
}

