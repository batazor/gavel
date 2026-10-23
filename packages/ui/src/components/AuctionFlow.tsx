import { useCallback, useState } from 'react';
import { addEdge, Background, Controls, Handle, Position, ReactFlow, applyEdgeChanges, applyNodeChanges, type Connection, type Edge, type EdgeChange, type Node, type NodeChange, type NodeProps } from '@xyflow/react';
import { Check, Clock3, CreditCard, Gavel, Mail } from 'lucide-react';
import { cn } from '../lib/cn';

type AuctionNodeData = {
  label: string;
  detail: string;
  state: 'done' | 'active' | 'waiting';
  kind: 'auction' | 'timer' | 'payment' | 'notify';
};

const icons = { auction: Gavel, timer: Clock3, payment: CreditCard, notify: Mail };

/* A builder node: the type's glyph in a neutral tile (the one place a boxed icon codes something), the active node in the hue. */
function AuctionNode({ data }: NodeProps<Node<AuctionNodeData>>) {
  const Icon = icons[data.kind];
  return (
    <div className={cn('min-w-48 rounded-[8px] border bg-surface p-3', data.state === 'active' ? 'border-copper' : 'border-line')}>
      <Handle type="target" position={Position.Left} className="!size-2 !border-0 !bg-control" />
      <div className="flex items-start gap-3">
        <span className={cn('grid size-8 shrink-0 place-items-center rounded-[6px]', data.state === 'done' ? 'bg-ink text-surface' : 'bg-raised text-muted')}>
          {data.state === 'done' ? <Check size={16} /> : <Icon size={16} strokeWidth={1.5} />}
        </span>
        <div>
          <p className="text-sm font-medium text-ink">{data.label}</p>
          <p className="mt-0.5 text-xs text-muted">{data.detail}</p>
        </div>
      </div>
      <Handle type="source" position={Position.Right} className="!size-2 !border-0 !bg-control" />
    </div>
  );
}

const initialNodes: Array<Node<AuctionNodeData>> = [
  { id: 'close', position: { x: 20, y: 90 }, data: { label: 'Auction closed', detail: 'Hammer price €42,800', state: 'done', kind: 'auction' }, type: 'auction' },
  { id: 'settle', position: { x: 285, y: 20 }, data: { label: 'Settlement window', detail: 'Candidate 1 · 1 h 42 m left', state: 'active', kind: 'timer' }, type: 'auction' },
  { id: 'pay', position: { x: 285, y: 165 }, data: { label: 'Authorise payment', detail: 'Provider · waiting', state: 'waiting', kind: 'payment' }, type: 'auction' },
  { id: 'notify', position: { x: 550, y: 90 }, data: { label: 'Notify parties', detail: 'Email and in-app', state: 'waiting', kind: 'notify' }, type: 'auction' },
];

const initialEdges: Edge[] = [
  { id: 'close-settle', source: 'close', target: 'settle', animated: true, style: { stroke: '#9f4428' } },
  { id: 'settle-pay', source: 'settle', target: 'pay', style: { stroke: '#cdc7bc' } },
  { id: 'pay-notify', source: 'pay', target: 'notify', style: { stroke: '#cdc7bc' } },
];

const nodeTypes = { auction: AuctionNode };

export interface AuctionFlowProps { className?: string }

export function AuctionFlow({ className }: AuctionFlowProps) {
  const [nodes, setNodes] = useState(initialNodes);
  const [edges, setEdges] = useState(initialEdges);
  const onNodesChange = useCallback((changes: NodeChange<Node<AuctionNodeData>>[]) => setNodes((current) => applyNodeChanges(changes, current)), []);
  const onEdgesChange = useCallback((changes: EdgeChange[]) => setEdges((current) => applyEdgeChanges(changes, current)), []);
  const onConnect = useCallback((connection: Connection) => setEdges((current) => addEdge(connection, current)), []);

  return (
    <div data-ui className={cn('h-[360px] min-w-0 overflow-hidden rounded-[8px] border border-line bg-ground', className)}>
      <ReactFlow nodes={nodes} edges={edges} nodeTypes={nodeTypes} onNodesChange={onNodesChange} onEdgesChange={onEdgesChange} onConnect={onConnect} fitView minZoom={0.65} maxZoom={1.5}>
        <Background color="#cdc7bc" gap={24} size={1} />
        <Controls position="bottom-right" showInteractive={false} />
      </ReactFlow>
    </div>
  );
}
