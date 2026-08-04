import { withBase } from '@rspress/core/runtime';
import { ArrowRight, GithubLogo } from '@phosphor-icons/react';
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
  title: string;
};

function SectionHeading({ body, title }: SectionHeadingProps) {
  return (
    <header className="cloud-section-heading" data-reveal>
      <h2>{title}</h2>
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
            <strong>{capability.title}</strong>: {capability.body}
          </li>
        ))}
      </ul>
      <h2>Product horizons</h2>
      <ul>
        {futureTracks.map((track) => (
          <li key={track.gate}>
            <strong>
              {track.gate}: {track.title}
            </strong>
            {`: ${track.body}`}
          </li>
        ))}
      </ul>
      <h2>Roadmap</h2>
      <ul>
        {roadmapGates.map((gate) => (
          <li key={gate.code}>
            <strong>
              {gate.code}: {gate.name}
            </strong>
            {`: ${gate.status}. ${gate.outcome}`}
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
          <h1>
            Declare intent.
            <span>Cloud converges.</span>
          </h1>
          <p>
            A self-hosted control plane that persists desired state and resumes
            every operation from durable evidence.
          </p>
          <div className="cloud-hero-actions">
            <a className="cloud-button is-primary" href="#control-loop">
              <span>Control loop</span>
              <ArrowRight aria-hidden="true" weight="bold" />
            </a>
            <a
              className="cloud-button is-secondary"
              href={route('/architecture/')}
            >
              Architecture
              <ArrowRight aria-hidden="true" weight="bold" />
            </a>
          </div>
        </div>
        <div className="cloud-hero-visual">
          <CloudTopology />
        </div>
      </section>

      <aside
        aria-label="Current delivery evidence and platform principles"
        className="cloud-assurance-bar"
      >
        <div className="cloud-assurance-status">
          <span>Delivery gates</span>
          <GateBadge gate={gateByCode('F0')} />
          <GateBadge gate={gateByCode('BX0')} />
        </div>
        <ul>
          <li>PostgreSQL truth through A3S ORM</li>
          <li>Outbound node control</li>
          <li>Box-only execution</li>
          <li>Gateway stays on the traffic path</li>
        </ul>
      </aside>

      <section className="cloud-section cloud-control-loop" id="control-loop">
        <SectionHeading
          body="An API request records intent; it does not impersonate deployment work. Every transition emits durable evidence and can continue after interruption."
          title="From desired state to observed truth"
        />
        <ConvergenceLoop />
      </section>

      <section className="cloud-section cloud-capabilities" id="capabilities">
        <SectionHeading
          body="Core capabilities reuse one application model, one persistence boundary, and one Runtime path. Each status is read from the repository roadmap at build time."
          title="One loop, not a pile of mechanisms"
        />
        <CapabilityGrid />
      </section>

      <section className="cloud-section cloud-boundaries" id="boundaries">
        <SectionHeading
          body="Cloud decides and records. Box executes. Gateway serves live traffic. Power becomes one typed inference backend; none of them becomes a second control plane."
          title="Clear authority at every hop"
        />
        <BoundaryMap />
      </section>

      <section className="cloud-section cloud-future" id="horizons">
        <SectionHeading
          body="Planned and in-progress capabilities are visible here by design. Their badges preserve the formal roadmap state instead of presenting them as shipped."
          title="What the control loop unlocks"
        />
        <FutureHorizons />
      </section>

      <section className="cloud-section cloud-roadmap" id="roadmap">
        <SectionHeading
          body="Filter the complete delivery matrix. Historical gates retain useful evidence but still require Box re-certification before they can be presented as verified."
          title="Every promise has a gate"
        />
        <RoadmapConstellation />
      </section>

      <section className="cloud-section cloud-architecture-cta" data-reveal>
        <a className="cloud-architecture-card" href={route('/architecture/')}>
          <div className="cloud-architecture-copy">
            <h2>See the whole Cloud in motion.</h2>
            <p>
              Orbit the 3D control plane, inspect exact ownership boundaries,
              and replay deployment, source delivery, traffic, recovery, and
              planned inference journeys.
            </p>
            <strong>
              Architecture
              <ArrowRight aria-hidden="true" weight="bold" />
            </strong>
          </div>
          <ArchitecturePreview />
        </a>
      </section>

      <section className="cloud-final-cta">
        <div>
          <h2>Make desired state durable.</h2>
          <p>
            Start with the current contract, then follow evidence over claims.
          </p>
        </div>
        <div>
          <a
            className="cloud-button is-primary"
            href="https://github.com/A3S-Lab/Cloud"
          >
            <GithubLogo aria-hidden="true" weight="fill" />
            GitHub
          </a>
          <a className="cloud-button is-secondary" href={route('/docs/')}>
            Docs
            <ArrowRight aria-hidden="true" weight="bold" />
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
          <a href="https://github.com/A3S-Lab/Cloud">GitHub</a>
        </div>
      </footer>
    </main>
  );
}
