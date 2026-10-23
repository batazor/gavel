import type { Meta, StoryObj } from '@storybook/react-vite';
import { Kpi, KpiStrip, MetricCard } from './MetricCard';

const meta = {
  title: 'Components/KPI',
  component: Kpi,
  parameters: { layout: 'centered', docs: { description: { component: 'A KPI is type, not a tile: a grey label over a figure at 400–500, the change in words with its window. No icon, no sparkline. Colour on a change says good or bad, not up or down; a zero or undefined change stays grey. Peers share one container split by 1px dividers.' } } },
  decorators: [(Story) => <div className="w-[min(960px,95vw)]"><Story /></div>],
  args: { label: 'Bids per minute', value: '2,418', change: '+186 vs yesterday 14:32', changeGood: true },
} satisfies Meta<typeof Kpi>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Single: Story = { render: (args) => <div className="w-[300px]"><MetricCard {...args} /></div> };

export const Strip: Story = {
  render: () => (
    <KpiStrip>
      <Kpi label="Bids per minute" value="2,418" change="+186 vs yesterday 14:32" changeGood lead />
      <Kpi label="Live auctions" value="128" change="+10 vs yesterday 14:32" changeGood />
      <Kpi label="Hammer volume today" value="€1,842,600" change="−€48,800 vs yesterday 14:32" changeGood={false} money />
      <Kpi label="Bidders, last 30 min" value="6,204" change="—" />
    </KpiStrip>
  ),
};

export const OddCountOnAPhone: Story = {
  parameters: { viewport: { defaultViewport: 'mobile1' } },
  render: () => (
    <KpiStrip>
      <Kpi label="Registered members" value="24,812" hint="all regions" />
      <Kpi label="Ready to bid" value="18,405" hint="74.2% of members" />
      <Kpi label="Waiting for review" value="38" hint="12 high priority" />
    </KpiStrip>
  ),
};

export const SingleCard: Story = {
  render: () => <div className="w-[300px]"><MetricCard label="Failed payments" value="0.42%" change="+0.08 pt vs last week" changeGood={false} /></div>,
};

export const ZerosAndDashes: Story = {
  render: () => (
    <KpiStrip>
      <Kpi label="Disputes open" value="0" change="— (none last week either)" />
      <Kpi label="Refunds today" value="€0" change="+€0 vs yesterday" money />
      <Kpi label="New sellers" value="4" change="+4 (new)" />
    </KpiStrip>
  ),
};
