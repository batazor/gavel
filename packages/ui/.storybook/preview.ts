import type { Preview } from '@storybook/react-vite';
import { MINIMAL_VIEWPORTS } from 'storybook/viewport';
import '../src/styles.css';

// Storybook's own placeholder headings ("No Preview", the error display) sit before the story root, so tooling that reads
// the first h1 would read them as the screen's title. Put the story root first in the document instead.
if (typeof document !== 'undefined') { const root = document.getElementById('storybook-root'); if (root) document.body.prepend(root); }

const gavelViewports = {
  ...MINIMAL_VIEWPORTS,
  tabletCompact: { name: 'Tablet compact · 768 × 1024', styles: { width: '768px', height: '1024px' }, type: 'tablet' as const },
  tabletComfortable: { name: 'Tablet comfortable · 834 × 1194', styles: { width: '834px', height: '1194px' }, type: 'tablet' as const },
  tabletLandscape: { name: 'Tablet landscape · 1024 × 768', styles: { width: '1024px', height: '768px' }, type: 'tablet' as const },
  desk: { name: 'Operator desk · 1440 × 900', styles: { width: '1440px', height: '900px' }, type: 'desktop' as const },
};

const preview: Preview = {
  parameters: {
    actions: { argTypesRegex: '^on[A-Z].*' },
    controls: { matchers: { color: /(background|color)$/i, date: /Date$/i } },
    backgrounds: {
      default: 'ground',
      values: [
        { name: 'ground', value: '#f4f2ee' },
        { name: 'surface', value: '#fffefc' },
      ],
    },
    viewport: { options: gavelViewports },
    layout: 'centered',
  },
  tags: ['autodocs'],
};

export default preview;
