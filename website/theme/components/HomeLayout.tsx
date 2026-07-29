import { withBase } from '@rspress/core/runtime';
import { AmbientGrid } from './AmbientGrid';
import { BoundaryMap } from './BoundaryMap';
import { CapabilityGrid } from './CapabilityGrid';
import { CloudTopology } from './CloudTopology';
import { ConvergenceLoop } from './ConvergenceLoop';
import { FutureHorizons } from './FutureHorizons';
import { GateBadge } from './GateBadge';
import { InteractionLayer } from './InteractionLayer';
import { RoadmapConstellation } from './RoadmapConstellation';
import { capabilities, futureTracks } from '../data/product';
import { gateByCode, roadmapGates } from '../data/roadmap';

type SectionHeadingProps = {
  body: string;
  eyebrow: string;
  title: string;
};

function ArrowIcon() {
  return (
    <svg aria-hidden="true" viewBox="0 0 20 20">
      <path d="M4 10h11M10.5 5.5 15 10l-4.5 4.5" />
    </svg>
  );
}

function GitHubIcon() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24">
      <path d="M12 2.8a9.4 9.4 0 0 0-3 18.3c.5.1.7-.2.7-.5v-1.8c-2.8.6-3.4-1.2-3.4-1.2-.5-1.2-1.1-1.5-1.1-1.5-.9-.6.1-.6.1-.6 1 0 1.6 1 1.6 1 .9 1.6 2.4 1.1 2.9.9.1-.7.4-1.1.6-1.4-2.2-.3-4.6-1.1-4.6-4.7 0-1 .4-1.9 1-2.6-.1-.3-.4-1.3.1-2.6 0 0 .8-.3 2.7 1a9.4 9.4 0 0 1 4.9 0c1.8-1.3 2.7-1 2.7-1 .5 1.3.2 2.3.1 2.6.6.7 1 1.6 1 2.6 0 3.6-2.3 4.4-4.5 4.7.4.3.7.9.7 1.8v2.7c0 .4.2.6.7.5A9.4 9.4 0 0 0 12 2.8Z" />
    </svg>
  );
}

function SectionHeading({ body, eyebrow, title }: SectionHeadingProps) {
  return (
    <header className="cloud-section-heading" data-reveal>
      <div>
        <span>{eyebrow}</span>
        <h2>{title}</h2>
      </div>
      <p>{body}</p>
    </header>
  );
}

function ArchitecturePreview() {
  return (
    <div className="cloud-architecture-preview" aria-hidden="true">
      <div className="cloud-architecture-plane is-plane-3">
        <span>Experience</span>
        <i />
        <i />
      </div>
      <div className="cloud-architecture-plane is-plane-2">
        <span>Control plane</span>
        <i />
        <i />
        <i />
      </div>
      <div className="cloud-architecture-plane is-plane-1">
        <span>Managed nodes</span>
        <i />
        <i />
      </div>
      <b className="cloud-architecture-beam" />
    </div>
  );
}

function MarkdownHome() {
  return (
    <main>
      <h1>A3S Cloud</h1>
      <p>
        A self-hosted desired-state control plane that persists intent and
        converges durable workloads, delivery, reachability, and operations on
        infrastructure you own.
      </p>
      <h2>Control loop</h2>
      <ol>
        <li>Commit tenant-scoped A3S ACL through the A3S Boot API.</li>
        <li>Persist business truth through A3S ORM and PostgreSQL.</li>
        <li>Resume durable work through A3S Flow.</li>
        <li>Lease outbound commands to the assigned Node Agent.</li>
        <li>Apply exact Runtime units through the sole A3S Box provider.</li>
        <li>Observe health, logs, receipts, and Gateway snapshots.</li>
      </ol>
      <h2>Capabilities</h2>
      <ul>
        {capabilities.map((capability) => (
          <li key={capability.title}>
            <strong>{capability.title}</strong> — {capability.body}
          </li>
        ))}
      </ul>
      <h2>Product horizons</h2>
      <ul>
        {futureTracks.map((track) => (
          <li key={track.gate}>
            <strong>
              {track.gate}: {track.title}
            </strong>{' '}
            — {track.body}
          </li>
        ))}
      </ul>
      <h2>Roadmap</h2>
      <ul>
        {roadmapGates.map((gate) => (
          <li key={gate.code}>
            <strong>
              {gate.code}: {gate.name}
            </strong>{' '}
            — {gate.status}. {gate.outcome}
          </li>
        ))}
      </ul>
    </main>
  );
}

export function HomeLayout() {
  const route = (path: string) => withBase(path);

  if (import.meta.env.SSG_MD) return <MarkdownHome />;

  return (
    <main className="cloud-home">
      <InteractionLayer />
      <div className="cloud-global-grid" aria-hidden="true">
        <AmbientGrid className="cloud-global-grid-canvas" />
      </div>

      <section className="cloud-hero">
        <div className="cloud-hero-copy">
          <div className="cloud-hero-eyebrow">
            <i aria-hidden="true" />
            SELF-HOSTED DESIRED-STATE CONTROL PLANE
          </div>
          <h1>
            Declare the state.
            <span>Cloud converges the system.</span>
          </h1>
          <p>
            Persist intent once. Resume every operation. Run immutable A3S
            workloads through Box, publish exact Gateway policy, and keep the
            evidence needed to recover.
          </p>
          <div className="cloud-hero-actions">
            <a className="cloud-button is-primary" href="#control-loop">
              <span>Watch the control loop</span>
              <ArrowIcon />
            </a>
            <a
              className="cloud-button is-secondary"
              href={route('/architecture/')}
            >
              Explore architecture
              <ArrowIcon />
            </a>
          </div>
          <div
            className="cloud-hero-status"
            aria-label="Current delivery status"
          >
            <GateBadge gate={gateByCode('F0')} />
            <GateBadge gate={gateByCode('BX0')} />
          </div>
        </div>
        <div className="cloud-hero-visual">
          <CloudTopology />
        </div>
        <div className="cloud-hero-metrics" aria-label="Platform principles">
          <span>
            <b>01</b> PostgreSQL truth through A3S ORM
          </span>
          <span>
            <b>02</b> Outbound node control
          </span>
          <span>
            <b>03</b> Box-only execution
          </span>
          <span>
            <b>04</b> Gateway stays on the traffic path
          </span>
        </div>
      </section>

      <section className="cloud-section cloud-control-loop" id="control-loop">
        <SectionHeading
          body="An API request records intent; it does not impersonate deployment work. Every transition emits durable evidence and can continue after interruption."
          eyebrow="THE CONVERGENCE LOOP"
          title="From desired state to observed truth"
        />
        <ConvergenceLoop />
      </section>

      <section className="cloud-section cloud-capabilities" id="capabilities">
        <SectionHeading
          body="Core capabilities reuse one application model, one persistence boundary, and one Runtime path. Each status is read from the repository roadmap at build time."
          eyebrow="CAPABILITY SYSTEM"
          title="One loop, not a pile of mechanisms"
        />
        <CapabilityGrid />
      </section>

      <section className="cloud-section cloud-boundaries" id="boundaries">
        <SectionHeading
          body="Cloud decides and records. Box executes. Gateway serves live traffic. Power becomes one typed inference backend; none of them becomes a second control plane."
          eyebrow="PRODUCT BOUNDARIES"
          title="Clear authority at every hop"
        />
        <BoundaryMap />
      </section>

      <section className="cloud-section cloud-future" id="horizons">
        <SectionHeading
          body="Planned and in-progress capabilities are visible here by design. Their badges preserve the formal roadmap state instead of presenting them as shipped."
          eyebrow="NEXT HORIZONS"
          title="The platform that the same loop unlocks"
        />
        <FutureHorizons />
      </section>

      <section className="cloud-section cloud-roadmap" id="roadmap">
        <SectionHeading
          body="Filter the complete delivery matrix. Historical gates retain useful evidence but still require Box re-certification before they can be presented as verified."
          eyebrow="LIVE PRODUCT ROADMAP"
          title="Every promise has a gate"
        />
        <RoadmapConstellation />
      </section>

      <section className="cloud-section cloud-architecture-cta" data-reveal>
        <a className="cloud-architecture-card" href={route('/architecture/')}>
          <div className="cloud-architecture-copy">
            <span>INTERACTIVE SYSTEM MAP</span>
            <h2>See the whole Cloud in motion.</h2>
            <p>
              Orbit the 3D control plane, inspect exact ownership boundaries,
              and replay deployment, source delivery, traffic, recovery, and
              planned inference journeys.
            </p>
            <strong>
              Open interactive architecture <ArrowIcon />
            </strong>
          </div>
          <ArchitecturePreview />
        </a>
      </section>

      <section className="cloud-final-cta">
        <div>
          <span>BUILD ON INFRASTRUCTURE YOU OWN</span>
          <h2>Make desired state durable.</h2>
          <p>
            Start with the current contract, then follow evidence—not claims.
          </p>
        </div>
        <div>
          <a
            className="cloud-button is-primary"
            href="https://github.com/A3S-Lab/Cloud"
          >
            <GitHubIcon />
            View on GitHub
          </a>
          <a className="cloud-button is-secondary" href={route('/docs/')}>
            Documentation
            <ArrowIcon />
          </a>
        </div>
      </section>

      <footer className="cloud-footer">
        <a href={route('/')}>
          <img alt="" src={route('/a3s-cloud-mark.svg')} />
          A3S Cloud
        </a>
        <span>Self-hosted desired-state control for A3S.</span>
        <div>
          <a href={route('/architecture/')}>Architecture</a>
          <a href={route('/docs/')}>Docs</a>
          <a href="https://github.com/A3S-Lab/Cloud">GitHub ↗</a>
        </div>
      </footer>
    </main>
  );
}
