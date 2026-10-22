import type { Preview } from '@storybook/react-vite';
import { MINIMAL_VIEWPORTS } from 'storybook/viewport';
import '../src/styles.css';

const gavelViewports = {
  ...MINIMAL_VIEWPORTS,
  tabletCompact: {
    name: 'Tablet compact · 768 × 1024',
    styles: { width: '768px', height: '1024px' },
    type: 'tablet' as const,
  },
  tabletComfortable: {
    name: 'Tablet comfortable · 834 × 1194',
    styles: { width: '834px', height: '1194px' },
    type: 'tablet' as const,
  },
  tabletLandscape: {
    name: 'Tablet landscape · 1024 × 768',
    styles: { width: '1024px', height: '768px' },
    type: 'tablet' as const,
  },
};

const preview: Preview = {
  parameters: {
    actions: { argTypesRegex: '^on[A-Z].*' },
    controls: {
      matchers: {
        color: /(background|color)$/i,
        date: /Date$/i,
      },
    },
    backgrounds: {
      default: 'parchment',
      values: [
        { name: 'parchment', value: '#f5f0e8' },
        { name: 'white', value: '#ffffff' },
        { name: 'ink', value: '#171716' },
      ],
    },
    viewport: {
      options: gavelViewports,
    },
    layout: 'centered',
  },
  tags: ['autodocs'],
};

export default preview;
