export type Locale = "en" | "es";

export interface FeatureItem {
  title: string;
  desc: string;
}

export interface StepItem {
  num: number;
  title: string;
  desc: string;
}

export interface WhyItem {
  title: string;
  desc: string;
}

export interface SiteCopy {
  meta: {
    title: string;
    description: string;
  };
  nav: {
    download: string;
    viewSource: string;
  };
  hero: {
    title: string;
    subtitle: string;
    description: string;
    downloadNow: string;
    viewOnGitHub: string;
  };
  overview: {
    title: string;
    p1: string;
    p2: string;
  };
  features: {
    title: string;
    items: FeatureItem[];
  };
  whyAudra: {
    title: string;
    items: WhyItem[];
  };
  howItWorks: {
    title: string;
    steps: StepItem[];
  };
  installation: {
    title: string;
    gettingStarted: {
      title: string;
      p1: string;
      p2: string;
      p3: string;
    };
    buildCommands: {
      title: string;
      fromSource: string;
      debian: string;
      fedora: string;
    };
    code: {
      fromSource: string;
      debian: string;
      fedora: string;
    };
  };
  footer: {
    licensing: string;
    licensingPrefix: string;
    licensingSuffix: string;
    copyright: string;
    github: string;
    issues: string;
  };
  language: {
    label: string;
    en: string;
    es: string;
  };
  a11y: {
    skipToContent: string;
    screenshot1Alt: string;
    screenshot2Alt: string;
  };
}

export type Translations = Record<Locale, SiteCopy>;
