import type { Meta, StoryObj } from '@storybook/react-vite';
import { ProductPagesLab } from './ProductPagesLab';

const meta = { title: 'Prototype/Product pages', component: ProductPagesLab, parameters: { layout: 'fullscreen', docs: { description: { component: 'Throwaway product map: create lot, lots, customers, customer profile and admin overview. Switch with the floating arrows or `?variant=A|B|C|D|E`.' } } } } satisfies Meta<typeof ProductPagesLab>;
export default meta;
type Story = StoryObj<typeof meta>;
export const PageLab: Story = {};
