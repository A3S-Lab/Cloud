import { describe, expect, it } from 'bun:test';
import { CloudApi, type CloudFetch } from './api';
import {
  DEFAULT_AUTOMATION_DEFINITION_LIST_LIMIT,
  encodeAutomationDefinitionListOptions,
  MAXIMUM_AUTOMATION_DEFINITION_LIST_LIMIT,
  validateAutomationDefinitionListLimit,
  validateAutomationExpectedGeneration,
} from './automations';

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(
    JSON.stringify({
      code: status,
      message: 'Success',
      data,
      requestId: '019c0000-0000-7000-8000-000000000001',
      timestamp: '2026-08-28T00:00:00.000Z',
    }),
    { status, headers: { 'content-type': 'application/json' } }
  );
}

describe('Automation webhook and definition client surface', () => {
  it('uses exact management routes without Idempotency-Key', async () => {
    const calls: Array<Parameters<CloudFetch>> = [];
    const fetcher: CloudFetch = async (...args) => {
      calls.push(args);
      return jsonResponse({});
    };
    const api = new CloudApi('caller-token', '/api/v1', { fetch: fetcher });
    const org = 'organization / one';
    const project = 'project / one';
    const environment = 'environment / one';
    const endpointId = '019c0000-0000-7000-8000-000000000010';
    const automationId = '019c0000-0000-7000-8000-000000000011';
    const revisionId = '019c0000-0000-7000-8000-000000000012';
    const secretId = '019c0000-0000-7000-8000-000000000013';

    await api.createAutomationWebhookEndpoint(org, project, environment, {
      endpointId,
      endpointKey: 'release-hook',
      signingSecret: { secretId, version: 1 },
      maxBodyBytes: 65536,
      automationId,
      revisionId,
    });
    await api.getAutomationWebhookEndpoint(org, project, environment, endpointId);
    await api.disableAutomationWebhookEndpoint(org, project, environment, endpointId, {
      expectedGeneration: 1,
    });
    await api.enableAutomationWebhookEndpoint(org, project, environment, endpointId, {
      expectedGeneration: 2,
    });
    await api.revokeAutomationWebhookEndpoint(org, project, environment, endpointId, {
      expectedGeneration: 3,
    });
    await api.listAutomationDefinitions(org);
    await api.getAutomationDefinition(org, automationId);
    await api.getAutomationRevision(org, automationId, revisionId);

    const base =
      '/api/v1/organizations/organization%20%2F%20one/projects/project%20%2F%20one/environments/environment%20%2F%20one/automation-webhook-endpoints';
    expect(calls.map(([input]) => input)).toEqual([
      base,
      `${base}/${endpointId}`,
      `${base}/${endpointId}/disable`,
      `${base}/${endpointId}/enable`,
      `${base}/${endpointId}/revoke`,
      `/api/v1/organizations/organization%20%2F%20one/automation-definitions?limit=${DEFAULT_AUTOMATION_DEFINITION_LIST_LIMIT}`,
      `/api/v1/organizations/organization%20%2F%20one/automation-definitions/${automationId}`,
      `/api/v1/organizations/organization%20%2F%20one/automation-definitions/${automationId}/revisions/${revisionId}`,
    ]);
    for (const index of [0, 2, 3, 4]) {
      expect(calls[index]?.[1]).toEqual(
        expect.objectContaining({
          method: 'POST',
          headers: expect.not.objectContaining({ 'Idempotency-Key': expect.anything() }),
        })
      );
    }
    expect(calls[0]?.[1]?.body).toBe(
      JSON.stringify({
        endpointId,
        endpointKey: 'release-hook',
        signingSecret: { secretId, version: 1 },
        maxBodyBytes: 65536,
        automationId,
        revisionId,
      })
    );
  });

  it('bounds definition list options', () => {
    expect(encodeAutomationDefinitionListOptions()).toBe(
      `?limit=${DEFAULT_AUTOMATION_DEFINITION_LIST_LIMIT}`
    );
    expect(() => validateAutomationDefinitionListLimit(0)).toThrow(RangeError);
    expect(() => validateAutomationDefinitionListLimit(MAXIMUM_AUTOMATION_DEFINITION_LIST_LIMIT + 1)).toThrow(
      RangeError
    );
    expect(() => validateAutomationExpectedGeneration(-1)).toThrow(TypeError);
  });
});
