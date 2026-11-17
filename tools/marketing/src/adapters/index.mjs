import bluesky from './bluesky.mjs';
import mastodon from './mastodon.mjs';
import x from './x.mjs';
import youtube from './youtube.mjs';
import reddit from './reddit.mjs';
import discord from './discord.mjs';
import email from './email.mjs';
import tiktok from './tiktok.mjs';
import instagram from './instagram.mjs';

export const ADAPTERS = { bluesky, mastodon, x, youtube, reddit, discord, email, tiktok, instagram };

/** Why a platform goes to manual export instead of an API, or null. */
export function manualReason(platform, config) {
  if (platform === 'devlog') return 'the devlog is published by hand (or by your site generator)';
  const a = ADAPTERS[platform];
  if (!a) return 'no adapter';
  if (a.gated && !(config[platform]?.enabled && config[platform]?.appApproved))
    return `${platform} needs an approved developer app; set ${platform}.enabled and ${platform}.appApproved once you have one`;
  if (platform === 'email' && config.email.provider === 'manual') return 'email.provider is "manual"';
  return null;
}
