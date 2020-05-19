<?php

namespace Titan\Tests\Feature;

use Illuminate\Foundation\Testing\RefreshDatabase;
use Titan\Tests\TestCase;

class AdminDashboardTest extends TestCase
{
    use RefreshDatabase;

    public function testPageRedirectsToLoginWhenNotLoggedIn()
    {
        $this->followRedirects = false;

        $this->get(route('admin.home'))
            ->assertRedirect('/login')
            ->assertLocation(route('login'));
    }

    public function testPageLoadsWhenAuthorized()
    {

        [$user, $character] = $this->seedUser([
            'role'  =>  'Super Admin'
        ]);

        $req = $this->actingAs($user)
            ->get(route('admin.home'))
            ->assertViewIs('titan::admin.home');

    }
}
