import { Broadcast, Code, Factory, FlowArrow } from '@phosphor-icons/react';
import type { HomeLanguage, ProductId } from '../data/product';
import { AgentFactoryStory } from './product-motion/AgentFactoryStory';
import { GatewayStory } from './product-motion/GatewayStory';
import { WorkflowStory } from './product-motion/WorkflowStory';

export { HeroVisual } from './product-motion/HeroVisual';

type ChartProps = {
  language: HomeLanguage;
};

export function ProductChart({ id, language }: ChartProps & { id: ProductId }) {
  if (id === 'workflow') return <WorkflowStory language={language} />;
  if (id === 'agent-factory') return <AgentFactoryStory language={language} />;
  return <GatewayStory language={language} />;
}

export function ProductIcon({ id }: { id: ProductId }) {
  if (id === 'workflow')
    return <FlowArrow aria-hidden="true" weight="duotone" />;
  if (id === 'agent-factory')
    return <Factory aria-hidden="true" weight="duotone" />;
  return <Broadcast aria-hidden="true" weight="duotone" />;
}

export function HarnessIcon() {
  return <Code aria-hidden="true" weight="duotone" />;
}
