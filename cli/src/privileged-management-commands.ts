import {
  type CloudApi,
  PLATFORM_ROLE_POLICY_MAX_ACL_BYTES,
  type PlatformRole,
  TENANT_SUPPORT_GRANT_MAX_ACL_BYTES,
  validatePlatformRole,
  validateTenantSupportContractDigest,
} from '@a3s/cloud-client';
import { readAclDocument, requireAclMutationCommand } from './acl-file';
import type { ParsedArguments } from './arguments';
import {
  positionalPositiveSafeInteger,
  positionalUuid,
  requireMutationCommand,
  requireReadCommand,
  requireVersionedMutationCommand,
} from './command-options';
import { inputValidationUsageError } from './errors';
import {
  platformRoleBindingMutationResult,
  platformRoleBindingResult,
  platformRolePolicyMutationResult,
  platformRolePolicyResult,
  tenantSupportGrantApprovalMutationResult,
  tenantSupportGrantMutationResult,
  tenantSupportGrantProposalMutationResult,
  tenantSupportGrantResult,
} from './privileged-management-results';
import type { CommandResult } from './results';

export interface PrivilegedManagementCommandDependencies {
  readFile?: (path: string) => Promise<Uint8Array>;
}

export async function executePrivilegedManagementCommand(
  command: string,
  arguments_: ParsedArguments,
  cloudApi: () => CloudApi,
  dependencies: PrivilegedManagementCommandDependencies = {}
): Promise<CommandResult | undefined> {
  const { positionals } = arguments_;
  switch (command) {
    case 'platform-role-policy current':
      requireReadCommand(arguments_, 'platform-role-policy current', 2);
      return platformRolePolicyResult(await cloudApi().getCurrentPlatformRolePolicy());
    case 'platform-role-policy get':
      requireReadCommand(arguments_, 'platform-role-policy get <revision-id>');
      return platformRolePolicyResult(
        await cloudApi().getPlatformRolePolicyRevision(
          positionalUuid(positionals, 2, 'platform role policy revision ID')
        )
      );
    case 'platform-role-policy accept': {
      const mutation = requireAclMutationCommand(
        arguments_,
        4,
        'platform-role-policy accept <revision-number> <expected-current-revision-id>'
      );
      const canonicalAcl = await readAclDocument(
        mutation.file,
        { label: 'platform role policy ACL', maximumBytes: PLATFORM_ROLE_POLICY_MAX_ACL_BYTES },
        dependencies.readFile
      );
      return platformRolePolicyMutationResult(
        await cloudApi().acceptPlatformRolePolicy(
          {
            canonicalAcl,
            revisionNumber: positionalPositiveSafeInteger(
              positionals,
              2,
              'platform role policy revision number'
            ),
            expectedCurrentRevisionId: positionalUuid(
              positionals,
              3,
              'expected current platform role policy revision ID'
            ),
          },
          mutation.idempotencyKey
        )
      );
    }
    case 'platform-role-bindings get':
      requireReadCommand(arguments_, 'platform-role-bindings get <binding-id>');
      return platformRoleBindingResult(
        await cloudApi().getPlatformRoleBinding(positionalUuid(positionals, 2, 'platform role binding ID'))
      );
    case 'platform-role-bindings get-principal':
      requireReadCommand(arguments_, 'platform-role-bindings get-principal <principal-id>');
      return platformRoleBindingResult(
        await cloudApi().getPrincipalPlatformRoleBinding(positionalUuid(positionals, 2, 'principal ID'))
      );
    case 'platform-role-bindings create': {
      const idempotencyKey = requireMutationCommand(
        arguments_,
        5,
        'platform-role-bindings create <principal-id> <role> <expected-policy-revision-id>'
      );
      return platformRoleBindingMutationResult(
        await cloudApi().createPlatformRoleBinding(
          {
            principalId: positionalUuid(positionals, 2, 'principal ID'),
            role: platformRole(positionals[3]),
            expectedPolicyRevisionId: positionalUuid(
              positionals,
              4,
              'expected platform role policy revision ID'
            ),
          },
          idempotencyKey
        )
      );
    }
    case 'platform-role-bindings change-role': {
      const mutation = requireVersionedMutationCommand(
        arguments_,
        5,
        'platform-role-bindings change-role <binding-id> <role> <expected-policy-revision-id>',
        'platform role binding'
      );
      return platformRoleBindingMutationResult(
        await cloudApi().changePlatformRoleBinding(
          positionalUuid(positionals, 2, 'platform role binding ID'),
          {
            role: platformRole(positionals[3]),
            expectedVersion: mutation.expectedVersion,
            expectedPolicyRevisionId: positionalUuid(
              positionals,
              4,
              'expected platform role policy revision ID'
            ),
          },
          mutation.idempotencyKey
        )
      );
    }
    case 'platform-role-bindings revoke': {
      const mutation = requireVersionedMutationCommand(
        arguments_,
        3,
        'platform-role-bindings revoke <binding-id>',
        'platform role binding'
      );
      return platformRoleBindingMutationResult(
        await cloudApi().revokePlatformRoleBinding(
          positionalUuid(positionals, 2, 'platform role binding ID'),
          mutation.expectedVersion,
          mutation.idempotencyKey
        )
      );
    }
    case 'tenant-support-grants get':
      requireReadCommand(arguments_, 'tenant-support-grants get <grant-id>');
      return tenantSupportGrantResult(
        await cloudApi().getTenantSupportGrant(positionalUuid(positionals, 2, 'tenant-support grant ID'))
      );
    case 'tenant-support-grants propose': {
      const mutation = requireAclMutationCommand(arguments_, 2, 'tenant-support-grants propose');
      const canonicalAcl = await readAclDocument(
        mutation.file,
        { label: 'tenant-support grant ACL', maximumBytes: TENANT_SUPPORT_GRANT_MAX_ACL_BYTES },
        dependencies.readFile
      );
      return tenantSupportGrantProposalMutationResult(
        await cloudApi().proposeTenantSupportGrant({ canonicalAcl }, mutation.idempotencyKey)
      );
    }
    case 'tenant-support-grants approve': {
      const idempotencyKey = requireMutationCommand(
        arguments_,
        4,
        'tenant-support-grants approve <grant-id> <expected-contract-digest>'
      );
      return tenantSupportGrantApprovalMutationResult(
        await cloudApi().approveTenantSupportGrant(
          positionalUuid(positionals, 2, 'tenant-support grant ID'),
          { expectedContractDigest: contractDigest(positionals[3]) },
          idempotencyKey
        )
      );
    }
    case 'tenant-support-grants revoke': {
      const mutation = requireVersionedMutationCommand(
        arguments_,
        3,
        'tenant-support-grants revoke <grant-id>',
        'tenant-support grant'
      );
      return tenantSupportGrantMutationResult(
        await cloudApi().revokeTenantSupportGrant(
          positionalUuid(positionals, 2, 'tenant-support grant ID'),
          mutation.expectedVersion,
          mutation.idempotencyKey
        )
      );
    }
    default:
      return undefined;
  }
}

function platformRole(value: string | undefined): PlatformRole {
  try {
    validatePlatformRole(value);
    return value;
  } catch (error) {
    throw inputValidationUsageError(error);
  }
}

function contractDigest(value: string | undefined): string {
  try {
    validateTenantSupportContractDigest(value);
    return value;
  } catch (error) {
    throw inputValidationUsageError(error);
  }
}
