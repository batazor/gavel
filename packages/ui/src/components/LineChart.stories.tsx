import type { Meta, StoryObj } from '@storybook/react-vite';
import { LineChart } from './LineChart';
import { Panel } from './Panel';
import { bidsLabels, bidsPerMinute, bidsSpike, hammerVolumeSeries } from '../data/ops';
import { formatClock, formatInt, formatMoney } from '../lib/format';

const meta = {
  title: 'Visualization/Line chart',
  component: LineChart,
  parameters: { layout: 'centered', docs: { description: { component: 'One series, 2px straight segments, no fill, no markers. Four faint gridlines with round ticks that top out above the maximum, 12px axis text, flat x labels on one step. The event that matters is annotated. The one hue goes on the screen\'s main series; a secondary chart stays in ink.' } } },
  decorators: [(Story) => <div className="w-[min(900px,95vw)]"><Story /></div>],
  args: { labels: bidsLabels.map(formatClock), values: bidsPerMinute, xAt: [0, 15, 30, 45, 60], format: formatInt, ariaLabel: 'Accepted bids per minute over the last hour' },
} satisfies Meta<typeof LineChart>;

export default meta;
type Story = StoryObj<typeof meta>;

export const MainSeries: Story = { args: { hue: 'copper', annotations: [{ at: bidsSpike.at, label: bidsSpike.label }] } };
export const SecondarySeries: Story = { args: { hue: 'ink', values: hammerVolumeSeries, format: (v: number) => formatMoney(v), ariaLabel: 'Hammer volume since midnight' } };
export const WithReference: Story = { args: { hue: 'copper', reference: { value: 2_600, label: 'Fan-out capacity' } } };
export const InAPanel: Story = {
  render: (args) => (
    <Panel title="Accepted bids" meta="13:32–14:32 UTC, one value per minute">
      <div className="px-5 pb-4"><LineChart {...args} hue="copper" annotations={[{ at: bidsSpike.at, label: bidsSpike.label }]} /></div>
    </Panel>
  ),
};
