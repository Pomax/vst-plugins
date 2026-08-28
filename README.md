# VST Plugins

This is a repository of proof-of-concept VST3 plugins that I needed and couldn't find a decent free or paid version for already.

## Plugins

There is currently one plugin here, aimed at project management.

### Markdown Notes

This is VST3 filter that lets you write sectioned markdown notes without having to save them to "a file" that you then need to bundle with your DAW project. It allows you to save and load markdown files if you want, but the plugin *is* your document. Just


And yes, some DAWs offer a way to save notes for projects already, either as a project settings thing or even as a simple VST, but none of them actually take the act of writing notes seriously. They offer a plain text area, typically a very small one, and leave you to figure out how that's useful. And most people end up going "it's not, I'm just going to take notes in a real text editor and save those with my project myself".

So this is a Proof of Concept plugin that shows how things could be better. Normal markdown editing, sections, saved directly into your project so you can never lose your notes again. And if you clone your project, good news, no manual nonsense for making a new notes document. It's already right there. So it's something you can just add to your templates so you always have notes already there for you to work with.

#### Sectioned on top level headings

The plugin has sections ("tabs") that are based on top level headings. If you load a markdown file with multiple `# ...` headings, the plugin will split that document up into separate sections for easy docs work. E.g. if you have a project briefing, and mixing instructions, and vocal directions, etc. you can just make each of those things their own section so you don't have to scroll past a million things just to work on specific notes.

#### WYSIWYG and source view

The plugin takes a what-you-see-is-what-you-get approach to markdown: you get to write it, but it only shows the markdown for things that your cursor is on. Anything else is simply styled text, because your reading experience should be text, not markdown syntax. But your _editing_ experience should absolutely be markdown syntax.

However, it also understands that sometimes you just want to quickly edit stuff without styling getting in the way. So there's a toggle button to go between styled text and plain markdown source.
