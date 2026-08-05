import { CheckCircle, GitBranch } from '@phosphor-icons/react';
import { gateByCode } from '../data/roadmap';
import {
  localize,
  type HomeLanguage,
  type ProductChapter as ProductChapterData,
} from '../data/product';
import { GateBadge } from './GateBadge';
import { ProductChart, ProductIcon } from './ProductCharts';

type ProductChapterProps = {
  language: HomeLanguage;
  product: ProductChapterData;
};

export function ProductChapter({ language, product }: ProductChapterProps) {
  const zh = language === 'zh';

  return (
    <section
      className={`cloud-product-chapter is-${product.id}`}
      id={product.id}
    >
      <div className="cloud-product-copy" data-reveal>
        <header>
          <span className="cloud-product-index">{product.index}</span>
          <span className="cloud-product-icon">
            <ProductIcon id={product.id} />
          </span>
          <div>
            <small>{zh ? '构建于' : 'BUILT ON'}</small>
            <strong>{product.basedOn}</strong>
          </div>
        </header>
        <h2>{localize(product.promise, language)}</h2>
        <h3>{localize(product.title, language)}</h3>
        <p>{localize(product.body, language)}</p>

        <ul className="cloud-product-capabilities">
          {product.capabilities.map((capability) => (
            <li key={capability.title.en}>
              <CheckCircle aria-hidden="true" size={20} weight="duotone" />
              <div>
                <strong>{localize(capability.title, language)}</strong>
                <span>{localize(capability.body, language)}</span>
              </div>
            </li>
          ))}
        </ul>

        <footer className="cloud-product-roadmap">
          <div>
            <GitBranch aria-hidden="true" size={18} />
            <span>{zh ? '对应路线 Gate' : 'Roadmap gates'}</span>
            {product.gateCodes.map((code) => (
              <GateBadge compact gate={gateByCode(code)} key={code} />
            ))}
          </div>
          <p>{localize(product.roadmapNote, language)}</p>
        </footer>
      </div>

      <div className="cloud-product-visual" data-reveal>
        <ProductChart id={product.id} language={language} />
      </div>
    </section>
  );
}
