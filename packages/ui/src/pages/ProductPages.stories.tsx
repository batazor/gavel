import type { Meta, StoryObj } from '@storybook/react-vite';
import { AdminOverviewPage } from './AdminOverviewPage';
import { CreateLotPage } from './CreateLotPage';
import { CustomerDetailPage } from './CustomerDetailPage';
import { CustomersManagementPage } from './CustomersManagementPage';
import { LotsManagementPage } from './LotsManagementPage';

const meta = { title: 'Pages/Product map', component: AdminOverviewPage, parameters: { layout: 'fullscreen' } } satisfies Meta<typeof AdminOverviewPage>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Overview: Story = {};

export const CreateLot: Story = { render: () => <CreateLotPage />, parameters: { docs: { description: { story: 'Four-step seller workflow with media, catalog details, pricing and review.' } } } };
export const Lots: Story = { render: () => <LotsManagementPage /> };
export const Customers: Story = { render: () => <CustomersManagementPage /> };
export const CustomerProfile: Story = { render: () => <CustomerDetailPage /> };
