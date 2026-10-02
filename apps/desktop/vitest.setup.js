import '@testing-library/jest-dom/vitest';
import { afterEach, vi } from 'vitest';
import { cleanup } from '@testing-library/svelte';

afterEach(() => {
  cleanup();
});


vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
