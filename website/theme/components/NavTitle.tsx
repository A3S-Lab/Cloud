import {
  addLeadingSlash,
  addTrailingSlash,
  normalizeImagePath,
  useLang,
  useSite,
  useVersion,
} from '@rspress/core/runtime';
import { Link } from '@rspress/core/theme';
import { useMemo } from 'react';

function documentationHome(
  language: string,
  defaultLanguage: string,
  version: string,
  defaultVersion: string,
) {
  const parts = [
    version && version !== defaultVersion ? version : '',
    language && language !== defaultLanguage ? language : '',
  ].filter(Boolean);
  return addTrailingSlash(addLeadingSlash(parts.join('/')));
}

export function NavTitle() {
  const { site } = useSite();
  const language = useLang();
  const version = useVersion();
  const defaultLanguage = site.lang ?? '';
  const defaultVersion = site.multiVersion.default ?? '';
  const locale = site.themeConfig.locales?.find(
    (candidate) => candidate.lang === language,
  );
  const title = (locale?.title ?? site.title) || 'A3S OS';
  const { logo: rawLogo, logoHref, logoText } = site;
  const logo = useMemo(() => {
    if (!rawLogo) return null;
    if (typeof rawLogo === 'string') {
      return (
        <img
          alt=""
          className="rspress-logo rp-nav__title__logo-image"
          id="logo"
          src={normalizeImagePath(rawLogo)}
        />
      );
    }
    return (
      <>
        <img
          alt=""
          className="rspress-logo rp-nav__title__logo-image rp-nav__title__logo-image--light"
          id="logo"
          src={normalizeImagePath(rawLogo.light)}
        />
        <img
          alt=""
          className="rspress-logo rp-nav__title__logo-image rp-nav__title__logo-image--dark"
          id="logo"
          src={normalizeImagePath(rawLogo.dark)}
        />
      </>
    );
  }, [rawLogo]);

  const home = documentationHome(
    language,
    defaultLanguage,
    version,
    defaultVersion,
  );

  return (
    <div className="rp-nav__title">
      <Link className="rp-nav__title__link" href={logoHref || home}>
        {logo && <div className="rp-nav__title__logo">{logo}</div>}
        {logoText && <span>{logoText}</span>}
        {!logo && !logoText && <span>{title}</span>}
      </Link>
    </div>
  );
}
