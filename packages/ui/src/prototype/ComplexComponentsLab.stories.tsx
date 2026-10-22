import type { Meta, StoryObj } from '@storybook/react-vite';
import { ComplexComponentsLab } from './ComplexComponentsLab';

const meta = {
  title: 'Prototype/Complex components',
  component: ComplexComponentsLab,
  parameters: { layout: 'fullscreen', docs: { description: { component: 'Throwaway lab for three domain-heavy components. Use the floating arrows or ←/→; selection is reflected in `?variant=A|B|C`.' } } },
} satisfies Meta<typeof ComplexComponentsLab>;

export default meta;
type Story = StoryObj<typeof meta>;
export const ComponentLab: Story = {};
