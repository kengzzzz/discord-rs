# Privacy policy

Last updated: 2026-11-11

This policy covers what data the discord-rs bot collects, why, and how long it keeps it. The bot's developer ("we") runs it on a server we control.

## What the bot reads

In the servers it is added to, the bot receives from Discord:

- server, channel and role IDs and names
- member user IDs, roles and join events
- message content, attachments and links in channels the bot can see
- voice channel joins, leaves and moves

The bot checks messages and images for spam and scams in memory as they arrive. It saves nothing from a message unless it flags the message (see Quarantine cases below).

## What the bot stores

| Data | Why | How long |
| --- | --- | --- |
| Server settings: channel IDs, role IDs and the IDs of the bot's own messages | So admins can set up the bot | Until an admin changes or removes them |
| Quarantine cases: user ID, the member's roles, the reason, a short excerpt of the flagged message or image text, and the time | So staff can review the case and release the member | Until staff release the member. If staff kick the member, the bot keeps the case and quarantines them again if they rejoin. |
| Your AI style, set with `/ai prompt set` | So the AI replies in that style | Until you run `/ai prompt clear` |
| Your recent AI conversation, both your messages and the AI's replies | So the AI remembers context | The last 12 turns at most. The bot deletes them after 24 hours, or at once when you run `/ai clear`. |
| Logs with user, channel and server IDs and errors | To fix bugs and stop abuse | A short time on our server |

The bot does not store your other messages, voice audio, or data from servers it is not in.

## Who else receives data

- Discord receives everything the bot sends, as with any bot.
- Google receives what you send to the AI through `/ai talk`, a mention of the bot, a `แปล` reply or Translate to Thai. That includes your text, attached images, your AI style and your recent AI conversation. Google handles it under the [Gemini API Terms](https://ai.google.dev/gemini-api/terms). On the free tier, Google may use this content to improve its products, and its human reviewers may read it. Do not send personal or sensitive information to the AI.
- Exa or Tavily receives a short search query that the AI writes from your question, when it needs to look something up. The query does not include your user ID.
- warframe.market and other public Warframe data services receive item names for lookups and no user data.

We do not sell your data or use it for ads.

## Your choices

- Run `/ai clear` to delete your AI history and `/ai prompt clear` to delete your AI style.
- Server admins can remove the bot at any time. Its settings for that server stay in the database until we delete them, which we do on request.
- Contact us to find out what data we hold about you or to have it deleted.

## Children

The bot is for users who meet Discord's minimum age. We do not knowingly collect data from younger children.

## Changes

We may update this policy. The date at the top shows the current version. We will announce large changes in the servers that use the bot.

## Contact

Open an issue at https://github.com/kengzzzz/discord-rs/issues, or message `._keng` on Discord for private requests.
