import { useCallback, useState } from 'react';
import {
  addEdge,
  Background,
  Controls,
  Handle,
  Position,
  ReactFlow,
  applyEdgeChanges,
  applyNodeChanges,
  type Connection,
  type Edge,
  type EdgeChange,
  type Node,
  type NodeChange,
  type NodeProps,
} from '@xyflow/react';
import { Check, Clock3, CreditCard, Gavel, Mail } from 'lucide-react';
import { cn } from '../lib/cn';

type AuctionNodeData = {
  label: string;
  detail: string;
  state: 'done' | 'active' | 'waiting';
  kind: 'auction' | 'timer' | 'payment' | 'notify';
};

const icons = { auction: Gavel, timer: Clock3, payment: CreditCard, notify: Mail };

function AuctionNode({ data }: NodeProps<Node<AuctionNodeData>>) {
  const Icon = icons[data.kind];
  return (
    <div className={cn(
      'min-w-48 rounded-xl border bg-paper-50 p-3 shadow-panel',
      data.state === 'active' ? 'border-copper-500 ring-4 ring-copper-500/10' : 'border-paper-200',
    )}>
      <Handle type="target" position={Position.Left} className="!size-2 !border-0 !bg-ink-500" />
      <div className="flex items-start gap-3">
        <span className={cn('grid size-8 shrink-0 place-items-center rounded-lg', data.state === 'done' ? 'bg-moss-500 text-white' : data.state === 'active' ? 'bg-copper-500 text-white' : 'bg-paper-200 text-ink-500')}>
          {data.state === 'done' ? <Check size={16} /> : <Icon size={16} />}
        </span>
        <div>
          <p className="m-0 text-xs font-bold text-ink-950">{data.label}</p>
          <p className="mt-1 mb-0 text-[10px] leading-4 text-ink-500">{data.detail}</p>
        </div>
      </div>
      <Handle type="source" position={Position.Right} className="!size-2 !border-0 !bg-ink-500" />
    </div>
  );
}

const initialNodes: Array<Node<AuctionNodeData>> = [
  { id: 'close', position: { x: 20, y: 90 }, data: { label: 'Auction closed', detail: 'Hammer price · €42,800', state: 'done', kind: 'auction' }, type: 'auction' },
  { id: 'settle', position: { x: 285, y: 20 }, data: { label: 'Settlement window', detail: 'Candidate 1 · 01:42 left', state: 'active', kind: 'timer' }, type: 'auction' },
  { id: 'pay', position: { x: 285, y: 165 }, data: { label: 'Authorize payment', detail: 'Stripe · waiting', state: 'waiting', kind: 'payment' }, type: 'auction' },
  { id: 'notify', position: { x: 550, y: 90 }, data: { label: 'Notify parties', detail: 'Email + in-app', state: 'waiting', kind: 'notify' }, type: 'auction' },
];

const initialEdges: Edge[] = [
  { id: 'close-settle', source: 'close', target: 'settle', animated: true, style: { stroke: '#bd5634' } },
  { id: 'settle-pay', source: 'settle', target: 'pay', style: { stroke: '#817a70' } },
  { id: 'pay-notify', source: 'pay', target: 'notify', style: { stroke: '#817a70' } },
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
    <div className={cn('h-[360px] min-w-0 overflow-hidden rounded-xl border border-paper-200 bg-paper-100', className)}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onConnect={onConnect}
        fitView
        minZoom={0.65}
        maxZoom={1.5}
      >
        <Background color="#cfc4b5" gap={24} size={1} />
        <Controls position="bottom-right" showInteractive={false} />
      </ReactFlow>
    </div>
  );
}
