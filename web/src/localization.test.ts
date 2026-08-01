import { describe, expect, test } from 'bun:test';

import {
  localizeNodeDescriptor,
  localizeWorkflow,
  nodeKindDescription,
  nodeKindLabel,
  statusLabel,
} from './localization';
import type { NodeKind, Workflow, WorkflowNode } from './types';

const kinds: NodeKind[] = [
  'start',
  'template',
  'llm',
  'agent',
  'tool',
  'router',
  'memory',
  'http',
  'approval',
  'output',
];

function node(id: string, label: string, config: unknown): WorkflowNode {
  return {
    id,
    type: id === 'start' ? 'start' : id === 'output' ? 'output' : 'template',
    position: { x: 0, y: 0 },
    data: {
      label,
      config,
      runtime: { secrets: [] },
    },
  };
}

function workflow(overrides: Partial<Workflow> = {}): Workflow {
  return {
    id: 'welcome-workflow',
    name: 'Welcome to A3S Workflow',
    description: 'A durable workflow executed by A3S Flow.',
    version: 1,
    nodes: [],
    edges: [],
    createdAt: '2026-08-01T00:00:00Z',
    updatedAt: '2026-08-01T00:00:00Z',
    ...overrides,
  };
}

describe('Chinese localization', () => {
  test('localizes every node kind and descriptor without changing its contract', () => {
    expect(kinds.map(nodeKindLabel)).toEqual([
      '开始',
      '模板转换',
      '大语言模型',
      '智能体',
      '工具',
      '条件分支',
      '记忆',
      'HTTP 请求',
      '人工审批',
      '结束',
    ]);
    expect(kinds.map(nodeKindDescription).every((description) => description.endsWith('。')))
      .toBe(true);

    expect(localizeNodeDescriptor({
      kind: 'agent',
      label: 'Agent',
      description: 'Agent node',
      defaultConfig: { iterations: 4 },
    })).toEqual({
      kind: 'agent',
      label: '智能体',
      description: '自主规划、推理并调用工具。',
      defaultConfig: { iterations: 4 },
    });
  });

  test('localizes known statuses and preserves unknown values', () => {
    expect(statusLabel()).toBe('未知');
    expect(statusLabel(null)).toBe('未知');
    expect(statusLabel('RUNNING')).toBe('运行中');
    expect(statusLabel('completed')).toBe('已完成');
    expect(statusLabel('custom-state')).toBe('custom-state');
  });

  test('translates the welcome workflow and its sample template immutably', () => {
    const source = workflow({
      nodes: [
        node('start', 'Input', null),
        node('greeting', 'Compose greeting', {
          value: { message: 'Hello, {{input.name}}!', keep: true },
          keep: true,
        }),
        node('output', 'Result', {}),
      ],
    });

    const localized = localizeWorkflow(source);

    expect(localized).not.toBe(source);
    expect(localized.name).toBe('欢迎使用 A3S Workflow');
    expect(localized.description).toBe('一个由 A3S Flow 持久化执行的工作流。');
    expect(localized.nodes.map((item) => item.data.label)).toEqual([
      '输入',
      '生成问候语',
      '结果',
    ]);
    expect(localized.nodes[1].data.config).toEqual({
      value: { message: '你好，{{input.name}}！', keep: true },
      keep: true,
    });
    expect(source.nodes[1].data.config).toEqual({
      value: { message: 'Hello, {{input.name}}!', keep: true },
      keep: true,
    });
  });

  test('leaves non-sample content and unsupported config shapes unchanged', () => {
    const unrelated = workflow({ id: 'customer-workflow' });
    expect(localizeWorkflow(unrelated)).toBe(unrelated);

    const source = workflow({
      name: 'Custom workflow',
      description: 'Custom description',
      nodes: [
        node('other', 'Custom node', { value: { message: 'Hello, {{input.name}}!' } }),
        node('greeting', 'Custom greeting', null),
        node('greeting', 'Custom greeting', 'text'),
        node('greeting', 'Custom greeting', {}),
        node('greeting', 'Custom greeting', { value: 'text' }),
        node('greeting', 'Custom greeting', { value: { message: 'Keep me' } }),
      ],
    });

    const localized = localizeWorkflow(source);

    expect(localized.name).toBe(source.name);
    expect(localized.description).toBe(source.description);
    expect(localized.nodes.map((item) => item.data.label)).toEqual(
      source.nodes.map((item) => item.data.label),
    );
    expect(localized.nodes.map((item) => item.data.config)).toEqual(
      source.nodes.map((item) => item.data.config),
    );
  });
});
