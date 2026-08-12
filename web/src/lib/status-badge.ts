export type StatusBadgeState = 'neutral' | 'active' | 'success' | 'warning' | 'danger';

export function statusBadgeState(value: string): StatusBadgeState {
  switch (value) {
    case 'active':
    case 'live':
    case 'queued':
    case 'preparing':
    case 'prepared':
    case 'scheduled':
    case 'running':
    case 'validating':
    case 'publishing':
    case 'attesting':
    case 'resolving':
    case 'applying':
    case 'verifying':
    case 'provisioning':
      return 'active';
    case 'succeeded':
    case 'healthy':
    case 'complete':
    case 'completed':
    case 'published':
    case 'ready':
    case 'verified':
      return 'success';
    case 'cancelling':
    case 'cleanup_pending':
    case 'pending':
    case 'draft':
    case 'retiring':
    case 'suspended':
    case 'issued':
    case 'connecting':
    case 'retrying':
    case 'in_progress':
    case 'in-progress':
    case 'recertification':
      return 'warning';
    case 'failed':
    case 'cancelled':
    case 'orphaned':
    case 'yanked':
    case 'rejected':
    case 'unavailable':
    case 'revoked':
    case 'unhealthy':
      return 'danger';
    default:
      return 'neutral';
  }
}
