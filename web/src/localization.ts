import type { NodeDescriptor, NodeKind, Workflow } from './types';

const NODE_LABELS: Record<NodeKind, string> = {
  start: '开始',
  template: '模板转换',
  llm: '大语言模型',
  agent: '智能体',
  tool: '工具',
  router: '条件分支',
  memory: '记忆',
  http: 'HTTP 请求',
  approval: '人工审批',
  output: '结束',
};

const NODE_DESCRIPTIONS: Record<NodeKind, string> = {
  start: '定义工作流的初始输入参数。',
  template: '使用结构化模板转换工作流变量。',
  llm: '调用大语言模型进行推理和生成。',
  agent: '自主规划、推理并调用工具。',
  tool: '使用工作流变量调用已安装的工具。',
  router: '根据类型化条件选择执行分支。',
  memory: '读取并持久化上下文记忆。',
  http: '向外部 HTTP 端点发送请求。',
  approval: '暂停执行并等待人工响应。',
  output: '返回工作流的最终输出变量。',
};

const STATUS_LABELS: Record<string, string> = {
  idle: '空闲',
  waiting: '等待中',
  active: '待处理',
  pending: '等待中',
  running: '运行中',
  succeeded: '成功',
  completed: '已完成',
  failed: '失败',
  cancelled: '已取消',
  error: '错误',
};

export function nodeKindLabel(kind: NodeKind): string {
  return NODE_LABELS[kind];
}

export function nodeKindDescription(kind: NodeKind): string {
  return NODE_DESCRIPTIONS[kind];
}

export function statusLabel(status?: string | null): string {
  if (!status) return '未知';
  return STATUS_LABELS[status.toLowerCase()] ?? status;
}

export function localizeNodeDescriptor(descriptor: NodeDescriptor): NodeDescriptor {
  return {
    ...descriptor,
    label: nodeKindLabel(descriptor.kind),
    description: nodeKindDescription(descriptor.kind),
  };
}

export function localizeWorkflow(source: Workflow): Workflow {
  if (source.id !== 'welcome-workflow') return source;

  return {
    ...source,
    name: source.name === 'Welcome to A3S Workflow' ? '欢迎使用 A3S Workflow' : source.name,
    description: source.description === 'A durable workflow executed by A3S Flow.'
      ? '一个由 A3S Flow 持久化执行的工作流。'
      : source.description,
    nodes: source.nodes.map((node) => {
      const labels: Record<string, [string, string]> = {
        start: ['Input', '输入'],
        greeting: ['Compose greeting', '生成问候语'],
        output: ['Result', '结果'],
      };
      const label = labels[node.id]?.[0] === node.data.label
        ? labels[node.id][1]
        : node.data.label;
      return {
        ...node,
        data: {
          ...node.data,
          label,
          config: localizeSampleConfig(node.id, node.data.config),
        },
      };
    }),
  };
}

function localizeSampleConfig(nodeId: string, source: unknown): unknown {
  if (nodeId !== 'greeting' || !source || typeof source !== 'object') return source;
  const config = source as Record<string, unknown>;
  if (!config.value || typeof config.value !== 'object') return source;
  const value = config.value as Record<string, unknown>;
  if (value.message !== 'Hello, {{input.name}}!') return source;
  return {
    ...config,
    value: {
      ...value,
      message: '你好，{{input.name}}！',
    },
  };
}
